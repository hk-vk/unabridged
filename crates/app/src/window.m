// Native window tweaks GPUI does not expose.
#import <AppKit/AppKit.h>

// Turns the window hosting `view` into a chromeless panel that floats above
// every app, on every Space and over full-screen apps, and stays visible
// while Better Audible is in the background.
void ba_window_float_everywhere(void *view) {
    NSView *ns_view = (__bridge NSView *)view;
    NSWindow *window = ns_view.window;
    if (window == nil) {
        return;
    }
    if ([window isKindOfClass:[NSPanel class]]) {
        ((NSPanel *)window).hidesOnDeactivate = NO;
        ((NSPanel *)window).floatingPanel = YES;
    }
    window.level = NSFloatingWindowLevel;
    window.collectionBehavior |= NSWindowCollectionBehaviorCanJoinAllSpaces |
                                 NSWindowCollectionBehaviorFullScreenAuxiliary;
    window.titleVisibility = NSWindowTitleHidden;
    window.titlebarAppearsTransparent = YES;
    for (NSWindowButton kind = NSWindowCloseButton; kind <= NSWindowZoomButton; kind++) {
        [[window standardWindowButton:kind] setHidden:YES];
    }
}
