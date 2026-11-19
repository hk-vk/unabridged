// Automatic updates through Sparkle, for release builds.
//
// Sparkle.framework is embedded in Contents/Frameworks by tools/build-release.sh
// and loaded here at runtime, so debug builds and tools/install-app.sh need
// neither the framework nor an update feed: without it, updates are simply
// unavailable and Settings hides the button.
#import <AppKit/AppKit.h>
#import <objc/message.h>

static id ba_updater_controller = nil;

// Starts Sparkle's standard updater (daily checks per Info.plist). Returns
// whether updates are available in this build.
bool ba_updater_start(void) {
    if (ba_updater_controller != nil) {
        return true;
    }
    NSString *frameworks = NSBundle.mainBundle.privateFrameworksPath;
    NSString *path = [frameworks stringByAppendingPathComponent:@"Sparkle.framework"];
    NSBundle *sparkle = [NSBundle bundleWithPath:path];
    if (sparkle == nil || ![sparkle load]) {
        return false;
    }
    // No feed in this bundle (a local build): stay off rather than error.
    if ([NSBundle.mainBundle objectForInfoDictionaryKey:@"SUFeedURL"] == nil) {
        return false;
    }
    Class controller = NSClassFromString(@"SPUStandardUpdaterController");
    if (controller == nil) {
        return false;
    }
    // -initWithStartingUpdater:updaterDelegate:userDriverDelegate:, called
    // directly with init's ownership rules (consumes self, returns +1).
    typedef id (*InitFn)(__attribute__((ns_consumed)) id, SEL, BOOL, id, id)
        __attribute__((ns_returns_retained));
    SEL init = NSSelectorFromString(@"initWithStartingUpdater:updaterDelegate:userDriverDelegate:");
    if (![controller instancesRespondToSelector:init]) {
        return false;
    }
    ba_updater_controller = ((InitFn)objc_msgSend)([controller alloc], init, YES, nil, nil);
    return ba_updater_controller != nil;
}

// "Check for Updates…": shows Sparkle's window with the result.
void ba_updater_check(void) {
    if (ba_updater_controller == nil) {
        return;
    }
    SEL check = NSSelectorFromString(@"checkForUpdates:");
    if ([ba_updater_controller respondsToSelector:check]) {
        ((void (*)(id, SEL, id))[ba_updater_controller methodForSelector:check])(
            ba_updater_controller, check, nil);
    }
}
