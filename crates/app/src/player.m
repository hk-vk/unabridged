#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>

void *ba_player_open(const char *path) {
    @autoreleasepool {
        NSString *file = [NSString stringWithUTF8String:path];
        if (!file) return NULL;
        NSError *error = nil;
        AVAudioPlayer *player = [[AVAudioPlayer alloc] initWithContentsOfURL:[NSURL fileURLWithPath:file] error:&error];
        if (!player || error) return NULL;
        player.enableRate = YES;
        return (__bridge_retained void *)player;
    }
}
void ba_player_close(void *handle) {
    if (handle) { AVAudioPlayer *player = (__bridge_transfer AVAudioPlayer *)handle; [player stop]; }
}
bool ba_player_play(void *handle) { return handle && [(__bridge AVAudioPlayer *)handle play]; }
void ba_player_pause(void *handle) { if (handle) [(__bridge AVAudioPlayer *)handle pause]; }
double ba_player_position(void *handle) { return handle ? [(__bridge AVAudioPlayer *)handle currentTime] : 0; }
double ba_player_duration(void *handle) { return handle ? [(__bridge AVAudioPlayer *)handle duration] : 0; }
void ba_player_seek(void *handle, double time) { if (handle) [(__bridge AVAudioPlayer *)handle setCurrentTime:time]; }
bool ba_player_is_playing(void *handle) { return handle && [(__bridge AVAudioPlayer *)handle isPlaying]; }
