// AVPlayer bridge for local files and streamed sources.
//
// Every ba_player_* function is called on the UI (main) thread. AVFoundation
// publishes item and player state through the main queue, which the app's run
// loop services; nothing here waits on it. Streamed bytes come from Rust through
// a read callback that runs on background queues.
#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreMedia/CoreMedia.h>
#include <stdatomic.h>
#include <string.h>

typedef int64_t (*ba_read_fn)(const void *ctx, uint64_t offset, uint8_t *buf, size_t len);
typedef void (*ba_release_fn)(const void *ctx);

static NSString *const BAStreamScheme = @"unabridged-stream";
static const NSUInteger BAStreamPiece = 256 * 1024;

// Owns the Rust context. Every outstanding read retains this object, so the
// context is released only after the player is gone and the last read returns.
@interface BAStreamSource : NSObject
@property (nonatomic, readonly) long long length;
@property (nonatomic, readonly, copy) NSString *contentType;
- (instancetype)initWithContext:(const void *)ctx read:(ba_read_fn)read release:(ba_release_fn)release length:(long long)length contentType:(NSString *)contentType;
- (int64_t)readAt:(uint64_t)offset into:(uint8_t *)buf length:(size_t)len;
- (void)close;
- (BOOL)isClosed;
// The most recent read failed and none has succeeded since.
- (void)setFailing:(BOOL)failing;
- (BOOL)isFailing;
@end

@implementation BAStreamSource {
    const void *_ctx;
    ba_read_fn _read;
    ba_release_fn _release;
    atomic_bool _closed;
    atomic_bool _failing;
}
- (instancetype)initWithContext:(const void *)ctx read:(ba_read_fn)read release:(ba_release_fn)release length:(long long)length contentType:(NSString *)contentType {
    if ((self = [super init])) {
        _ctx = ctx;
        _read = read;
        _release = release;
        _length = length;
        _contentType = [contentType copy];
        atomic_init(&_closed, false);
        atomic_init(&_failing, false);
    }
    return self;
}
- (int64_t)readAt:(uint64_t)offset into:(uint8_t *)buf length:(size_t)len { return _read(_ctx, offset, buf, len); }
- (void)close { atomic_store(&_closed, true); }
- (BOOL)isClosed { return atomic_load(&_closed); }
- (void)setFailing:(BOOL)failing {
    if (atomic_load(&_failing) != failing) atomic_store(&_failing, failing);
}
- (BOOL)isFailing { return atomic_load(&_failing); }
- (void)dealloc {
    if (_release) _release(_ctx);
}
@end

static NSError *BAStreamError(NSInteger code, NSString *message) {
    return [NSError errorWithDomain:@"Unabridged.Stream" code:code userInfo:@{NSLocalizedDescriptionKey: message}];
}

@interface BAStreamLoader : NSObject <AVAssetResourceLoaderDelegate>
@property (nonatomic, readonly) BAStreamSource *source;
@property (nonatomic, readonly) dispatch_queue_t queue;
@end

@implementation BAStreamLoader
- (instancetype)initWithSource:(BAStreamSource *)source {
    if ((self = [super init])) {
        _source = source;
        _queue = dispatch_queue_create("app.unabridged.stream-loader", DISPATCH_QUEUE_SERIAL);
    }
    return self;
}

- (BOOL)resourceLoader:(AVAssetResourceLoader *)loader shouldWaitForLoadingOfRequestedResource:(AVAssetResourceLoadingRequest *)request {
    if (![request.request.URL.scheme isEqualToString:BAStreamScheme]) return NO;
    BAStreamSource *source = self.source;
    if (source.isClosed) {
        [request finishLoadingWithError:BAStreamError(NSURLErrorCancelled, @"Playback was closed.")];
        return YES;
    }
    AVAssetResourceLoadingContentInformationRequest *info = request.contentInformationRequest;
    if (info) {
        info.contentType = source.contentType;
        info.contentLength = source.length;
        info.byteRangeAccessSupported = YES;
    }
    AVAssetResourceLoadingDataRequest *data = request.dataRequest;
    if (!data) {
        [request finishLoading];
        return YES;
    }
    long long start = data.currentOffset;
    long long end = data.requestsAllDataToEndOfResource ? source.length : data.requestedOffset + (long long)data.requestedLength;
    if (end > source.length) end = source.length;
    // Reads may block on the network, so they run off the delegate queue.
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        long long offset = start;
        while (offset < end) {
            if (request.isCancelled) return;
            if (source.isClosed) {
                [request finishLoadingWithError:BAStreamError(NSURLErrorCancelled, @"Playback was closed.")];
                return;
            }
            @autoreleasepool {
                size_t want = (size_t)MIN((long long)BAStreamPiece, end - offset);
                NSMutableData *piece = [NSMutableData dataWithLength:want];
                int64_t got = [source readAt:(uint64_t)offset into:piece.mutableBytes length:want];
                if (request.isCancelled) return;
                if (got < 0) {
                    [source setFailing:YES];
                    [request finishLoadingWithError:BAStreamError(1, @"Could not read the audio stream.")];
                    return;
                }
                [source setFailing:NO];
                if (got == 0) break;
                if ((size_t)got > want) got = (int64_t)want;
                piece.length = (NSUInteger)got;
                [request.dataRequest respondWithData:piece];
                offset += got;
            }
        }
        if (!request.isCancelled) [request finishLoading];
    });
    return YES;
}

- (void)resourceLoader:(AVAssetResourceLoader *)loader didCancelLoadingRequest:(AVAssetResourceLoadingRequest *)request {
    // The read loop checks isCancelled between pieces and stops on its own.
}
@end

@interface BAPlayer : NSObject
@property (nonatomic, strong) AVPlayer *player;
@property (nonatomic, strong) AVPlayerItem *item;
@property (nonatomic, strong) AVURLAsset *asset;
@property (nonatomic, strong) BAStreamLoader *loader;
@property (nonatomic) double knownDuration;
@property (nonatomic) float chosenRate;
@end

@implementation BAPlayer {
    // The seek in flight: position() reports its target until it completes.
    // Completion handlers may run on any thread.
    atomic_uint_fast64_t _seekGeneration;
    atomic_bool _seeking;
    _Atomic double _seekTarget;
}

- (instancetype)initWithAsset:(AVURLAsset *)asset loader:(BAStreamLoader *)loader {
    if ((self = [super init])) {
        _asset = asset;
        _loader = loader;
        _chosenRate = 1.0f;
        atomic_init(&_seekGeneration, 0);
        atomic_init(&_seeking, false);
        atomic_init(&_seekTarget, 0.0);
        _item = [AVPlayerItem playerItemWithAsset:asset];
        // TimeDomain is Apple's choice for speech. Spectral is meant for
        // music and makes voices warble and stutter above 1x.
        _item.audioTimePitchAlgorithm = AVAudioTimePitchAlgorithmTimeDomain;
        _player = [AVPlayer playerWithPlayerItem:_item];
        _player.actionAtItemEnd = AVPlayerActionAtItemEndPause;
        _player.defaultRate = 1.0f;
        if (loader) _player.automaticallyWaitsToMinimizeStalling = YES;
    }
    return self;
}

- (BOOL)failed {
    return self.item.status == AVPlayerItemStatusFailed || self.player.status == AVPlayerStatusFailed;
}

- (double)duration {
    if (self.knownDuration > 0) return self.knownDuration;
    CMTime time = self.item.duration;
    if (!CMTIME_IS_NUMERIC(time)) return 0;
    double seconds = CMTimeGetSeconds(time);
    return isfinite(seconds) && seconds > 0 ? seconds : 0;
}

// Waiting for data while the stream's reads fail.
- (BOOL)stalled {
    return self.loader.source.isFailing && !self.failed &&
        self.player.timeControlStatus == AVPlayerTimeControlStatusWaitingToPlayAtSpecifiedRate;
}

// Seconds of loaded media from the playhead onward.
- (double)bufferedAhead {
    double now = CMTimeGetSeconds(self.player.currentTime);
    if (!isfinite(now)) return 0;
    for (NSValue *value in self.item.loadedTimeRanges) {
        CMTimeRange range = value.CMTimeRangeValue;
        double start = CMTimeGetSeconds(range.start);
        double end = start + CMTimeGetSeconds(range.duration);
        if (now >= start - 0.5 && now < end) return end - now;
    }
    return 0;
}

// After a failed read AVPlayer keeps waiting "to minimize stalls" even with
// minutes already loaded, so play what is there. Once that runs out the item
// does not ask for data again; the caller reopens the stream to retry.
// Called from the polled accessors.
- (void)poll {
    if (!self.stalled) return;
    if (![self.player.reasonForWaitingToPlay isEqualToString:AVPlayerWaitingToMinimizeStallsReason]) return;
    double ahead = self.bufferedAhead;
    double duration = self.duration;
    BOOL toEnd = duration > 0 && CMTimeGetSeconds(self.player.currentTime) + ahead >= duration - 0.5;
    if (ahead >= 10.0 || (ahead > 0 && toEnd)) [self.player playImmediatelyAtRate:self.chosenRate];
}

- (double)position {
    if (atomic_load(&_seeking)) return atomic_load(&_seekTarget);
    double seconds = CMTimeGetSeconds(self.player.currentTime);
    return isfinite(seconds) && seconds > 0 ? seconds : 0;
}

- (void)seek:(double)seconds {
    uint_fast64_t generation = atomic_fetch_add(&_seekGeneration, 1) + 1;
    atomic_store(&_seekTarget, seconds);
    atomic_store(&_seeking, true);
    CMTime time = CMTimeMakeWithSeconds(seconds, 1000000);
    __weak BAPlayer *weakSelf = self;
    void (^done)(BOOL) = ^(BOOL __unused finished) {
        BAPlayer *strongSelf = weakSelf;
        if (!strongSelf) return;
        // A newer seek supersedes this one and keeps reporting its own target.
        if (atomic_load(&strongSelf->_seekGeneration) == generation) atomic_store(&strongSelf->_seeking, false);
    };
    @try {
        [self.player seekToTime:time toleranceBefore:kCMTimeZero toleranceAfter:kCMTimeZero completionHandler:done];
    } @catch (NSException *exception) {
        // Older systems refuse completion handlers before the item is ready.
        [self.player seekToTime:time toleranceBefore:kCMTimeZero toleranceAfter:kCMTimeZero];
        atomic_store(&_seeking, false);
    }
}

- (void)close {
    [self.loader.source close];
    [self.player pause];
    [self.player replaceCurrentItemWithPlayerItem:nil];
    [self.asset cancelLoading];
    BAStreamLoader *loader = self.loader;
    if (loader) {
        // Keep the delegate alive until any callback already on its queue returns.
        dispatch_async(loader.queue, ^{ (void)loader; });
    }
}
@end

static BAPlayer *BA(void *handle) { return (__bridge BAPlayer *)handle; }

void *ba_player_open(const char *path) {
    @autoreleasepool {
        NSString *file = [NSString stringWithUTF8String:path];
        if (!file) return NULL;
        NSURL *url = [NSURL fileURLWithPath:file];
        double duration = 0;
        AVAudioFile *audio = [[AVAudioFile alloc] initForReading:url error:nil];
        if (audio && audio.processingFormat.sampleRate > 0) duration = (double)audio.length / audio.processingFormat.sampleRate;
        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:url options:@{AVURLAssetPreferPreciseDurationAndTimingKey: @YES}];
        if (!(duration > 0)) {
            // AVAudioFile does not read every container AVPlayer plays; ask the
            // asset directly, which is quick for a local file.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            CMTime time = asset.duration;
#pragma clang diagnostic pop
            if (CMTIME_IS_NUMERIC(time)) duration = CMTimeGetSeconds(time);
        }
        if (!(duration > 0) || !isfinite(duration)) return NULL;
        BAPlayer *player = [[BAPlayer alloc] initWithAsset:asset loader:nil];
        player.knownDuration = duration;
        return (__bridge_retained void *)player;
    }
}

void *ba_player_open_stream(const void *ctx, ba_read_fn read, ba_release_fn release, uint64_t length, const char *content_type, double duration) {
    @autoreleasepool {
        static atomic_uint_fast64_t counter = 0;
        NSString *type = content_type ? [NSString stringWithUTF8String:content_type] : nil;
        // The source owns ctx from here on, even if opening fails below.
        BAStreamSource *source = [[BAStreamSource alloc] initWithContext:ctx read:read release:release length:(long long)length contentType:type ?: @""];
        if (!type.length || length == 0) return NULL;
        BOOL mp3 = [type isEqualToString:@"public.mp3"];
        NSString *extension = mp3 ? @"mp3" : @"m4b";
        uint_fast64_t n = atomic_fetch_add(&counter, 1) + 1;
        NSURL *url = [NSURL URLWithString:[NSString stringWithFormat:@"%@://book/%llu.%@", BAStreamScheme, (unsigned long long)n, extension]];
        // Precise timing would make AVFoundation scan a whole MP3 over the network.
        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:url options:@{AVURLAssetPreferPreciseDurationAndTimingKey: @(!mp3)}];
        BAStreamLoader *loader = [[BAStreamLoader alloc] initWithSource:source];
        [asset.resourceLoader setDelegate:loader queue:loader.queue];
        BAPlayer *player = [[BAPlayer alloc] initWithAsset:asset loader:loader];
        player.knownDuration = isfinite(duration) && duration > 0 ? duration : 0;
        return (__bridge_retained void *)player;
    }
}

void ba_player_close(void *handle) {
    if (!handle) return;
    @autoreleasepool {
        BAPlayer *player = (__bridge_transfer BAPlayer *)handle;
        [player close];
    }
}

bool ba_player_play(void *handle) {
    if (!handle) return false;
    @autoreleasepool {
        BAPlayer *player = BA(handle);
        if (player.failed) return false;
        player.player.defaultRate = player.chosenRate;
        [player.player play];
        return true;
    }
}

void ba_player_pause(void *handle) {
    if (!handle) return;
    @autoreleasepool { [BA(handle).player pause]; }
}

double ba_player_position(void *handle) {
    if (!handle) return 0;
    @autoreleasepool {
        BAPlayer *player = BA(handle);
        [player poll];
        return player.position;
    }
}

double ba_player_duration(void *handle) {
    if (!handle) return 0;
    @autoreleasepool { return BA(handle).duration; }
}

void ba_player_seek(void *handle, double time) {
    if (!handle) return;
    @autoreleasepool { [BA(handle) seek:time]; }
}

bool ba_player_is_playing(void *handle) {
    if (!handle) return false;
    @autoreleasepool {
        BAPlayer *wrapper = BA(handle);
        [wrapper poll];
        if (wrapper.failed) return false;
        AVPlayer *player = wrapper.player;
        return player.rate != 0 || player.timeControlStatus != AVPlayerTimeControlStatusPaused;
    }
}

bool ba_player_is_buffering(void *handle) {
    if (!handle) return false;
    @autoreleasepool {
        BAPlayer *player = BA(handle);
        return !player.failed && player.player.timeControlStatus == AVPlayerTimeControlStatusWaitingToPlayAtSpecifiedRate;
    }
}

bool ba_player_is_stalled(void *handle) {
    if (!handle) return false;
    @autoreleasepool { return BA(handle).stalled; }
}

float ba_player_rate(void *handle) { return handle ? BA(handle).chosenRate : 1.0f; }

void ba_player_set_rate(void *handle, float rate) {
    if (!handle) return;
    @autoreleasepool {
        BAPlayer *player = BA(handle);
        player.chosenRate = rate;
        player.player.defaultRate = rate;
        // Only a playing (or waiting) player takes the new rate now; a paused one stays paused.
        if (player.player.rate != 0) player.player.rate = rate;
    }
}

float ba_player_volume(void *handle) {
    if (!handle) return 0;
    @autoreleasepool { return BA(handle).player.volume; }
}

void ba_player_set_volume(void *handle, float volume) {
    if (!handle) return;
    @autoreleasepool { BA(handle).player.volume = volume; }
}

// A malloc'd description of the failure, or NULL. The caller frees it.
char *ba_player_error(void *handle) {
    if (!handle) return NULL;
    @autoreleasepool {
        BAPlayer *player = BA(handle);
        NSError *error = nil;
        if (player.item.status == AVPlayerItemStatusFailed) error = player.item.error;
        else if (player.player.status == AVPlayerStatusFailed) error = player.player.error;
        else return NULL;
        NSString *message = error.localizedDescription ?: @"Playback failed.";
        return strdup(message.UTF8String ?: "Playback failed.");
    }
}
