#import <Cocoa/Cocoa.h>
#include <stdio.h>
#include <unistd.h>

static NSWindow *fixtureWindow;
static NSTextView *fixtureText;

// Public, synthetic window with no selection: idle AX queries must not leak.
int main(void) {
    @autoreleasepool {
        NSApplication *app = NSApplication.sharedApplication;
        [app setActivationPolicy:NSApplicationActivationPolicyAccessory];
        NSWindow *window = fixtureWindow = [[NSWindow alloc]
            initWithContentRect:NSMakeRect(200, 200, 460, 180)
            styleMask:NSWindowStyleMaskTitled
            backing:NSBackingStoreBuffered defer:NO];
        window.title = @"BeeK memory regression fixture";
        NSTextView *text = fixtureText = [[NSTextView alloc] initWithFrame:window.contentView.bounds];
        text.string = @"BeeK memory fixture";
        [window.contentView addSubview:text];
        [window makeKeyAndOrderFront:nil];
        [window makeFirstResponder:text];
        [text setSelectedRange:NSMakeRange(0, 0)];
        [app activateIgnoringOtherApps:YES];
        dispatch_async(dispatch_get_main_queue(), ^{
            [app activateIgnoringOtherApps:YES];
            [window makeKeyAndOrderFront:nil];
            [window makeFirstResponder:text];
            [text setSelectedRange:NSMakeRange(0, 0)];
            printf("ready %d\n", getpid());
            fflush(stdout);
        });
        [app run];
    }
}
