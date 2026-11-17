// Native window tweaks GPUI does not expose.
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>

// Resizes the window hosting `view` to `height`, keeping its top edge where it
// is (or its bottom on the screen when there is no room below), on the sheet
// curve cubic-bezier(0.32, 0.72, 0, 1). A zero duration resizes at once.
void ba_window_set_height(void *view, double height, double duration) {
    NSView *ns_view = (__bridge NSView *)view;
    NSWindow *window = ns_view.window;
    if (window == nil) {
        return;
    }
    NSRect frame = window.frame;
    NSRect next = frame;
    next.size.height = height;
    next.origin.y = NSMaxY(frame) - height;
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    if (screen != nil && next.origin.y < NSMinY(screen.visibleFrame)) {
        next.origin.y = NSMinY(screen.visibleFrame);
    }
    if (duration <= 0) {
        [window setFrame:next display:YES];
        return;
    }
    [NSAnimationContext runAnimationGroup:^(NSAnimationContext *context) {
        context.duration = duration;
        context.timingFunction =
            [CAMediaTimingFunction functionWithControlPoints:0.32:0.72:0.0:1.0];
        [[window animator] setFrame:next display:YES];
    }];
}

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

// Measurement runs only: keeps the window "visible" to macOS, so it is still
// drawn, while the user cannot see or click it and it never takes focus.
void ba_window_ghost(void *view) {
    NSWindow *window = ((__bridge NSView *)view).window;
    if (window == nil) {
        return;
    }
    window.level = NSFloatingWindowLevel;
    window.alphaValue = 0.01;
    window.ignoresMouseEvents = YES;
    window.collectionBehavior |= NSWindowCollectionBehaviorCanJoinAllSpaces |
                                 NSWindowCollectionBehaviorFullScreenAuxiliary;
}
