#import <AppKit/AppKit.h>
#import <AVKit/AVKit.h>
#import <AVFoundation/AVFoundation.h>
#import <PDFKit/PDFKit.h>
#import <CoreMedia/CoreMedia.h>
#include <math.h>
#include <string.h>

static NSMutableSet *EGViewers;
static size_t EGMainWindowCount;
void explorer_set_main_window_count(size_t count) { EGMainWindowCount=count; }
static void *EGPlayerStatus = &EGPlayerStatus;
static NSURL *EGFileURL(const char *path) {
    NSString *name=[[NSFileManager defaultManager] stringWithFileSystemRepresentation:path length:strlen(path)];
    return [[NSURL fileURLWithPath:name] URLByResolvingSymlinksInPath];
}
static NSImage *EGLoadImage(NSURL *url,NSString **error) {
    NSImage *image=[[NSImage alloc] initWithContentsOfURL:url];
    if(!image || image.size.width<=0 || image.size.height<=0){*error=@"This image could not be decoded. Use Open externally to try another app.";return nil;}
    for(NSImageRep *rep in image.representations)if(rep.pixelsWide>0&&rep.pixelsHigh>0){image.size=NSMakeSize(rep.pixelsWide,rep.pixelsHigh);break;}
    return image;
}
static PDFDocument *EGLoadPDF(NSURL *url,NSString **error) {
    PDFDocument *document=[[PDFDocument alloc] initWithURL:url];
    if(!document){*error=@"This file could not be read as a PDF.";return nil;}
    return document;
}
// Also used by headless integration tests; no application/window is created.
size_t explorer_probe_media(const char *path,int kind,char **error) {
    @autoreleasepool {
        NSString *failure=nil;NSURL *url=EGFileURL(path);size_t result=0;
        if(kind==1){result=EGLoadImage(url,&failure)?1:0;}
        else if(kind==3){PDFDocument *pdf=EGLoadPDF(url,&failure);result=pdf.isLocked?1:pdf.pageCount;}
        else{failure=@"Unsupported probe kind";}
        if(!result)*error=strdup((failure?:@"No readable content").UTF8String);
        return result;
    }
}

@interface EGMediaWindow : NSWindow @end
@implementation EGMediaWindow
- (BOOL)performKeyEquivalent:(NSEvent*)event {
    if((event.modifierFlags & NSEventModifierFlagCommand)!=0){
        NSString *key=event.charactersIgnoringModifiers.lowercaseString;
        if([key isEqualToString:@"c"]&&[NSApp sendAction:@selector(copy:) to:nil from:self])return YES;
        if([key isEqualToString:@"v"]&&[NSApp sendAction:@selector(paste:) to:nil from:self])return YES;
        if([key isEqualToString:@"a"]&&[NSApp sendAction:@selector(selectAll:) to:nil from:self])return YES;
        if([key isEqualToString:@"w"]){[self performClose:nil];return YES;}
        if([key isEqualToString:@"q"]){[NSApp terminate:nil];return YES;}
    }
    return [super performKeyEquivalent:event];
}
@end

@interface EGCenteringClipView : NSClipView @end
@implementation EGCenteringClipView
- (NSRect)constrainBoundsRect:(NSRect)proposed {
    NSRect bounds=[super constrainBoundsRect:proposed];NSRect frame=self.documentView.frame;
    if(frame.size.width<bounds.size.width)bounds.origin.x=NSMidX(frame)-bounds.size.width/2;
    if(frame.size.height<bounds.size.height)bounds.origin.y=NSMidY(frame)-bounds.size.height/2;
    return bounds;
}
@end

@interface EGMediaController : NSWindowController<NSWindowDelegate>
@property NSURL *url;
@property NSInteger kind;
@property NSStackView *bar;
@property NSTextField *info;
@property NSScrollView *imageScroll;
@property NSImageView *imageView;
@property NSArray<NSURL*> *images;
@property NSUInteger imageIndex;
@property NSButton *previousButton;
@property NSButton *nextButton;
@property BOOL autoFit;
@property AVPlayerView *videoView;
@property AVPlayer *player;
@property AVPlayerItem *observedItem;
@property BOOL observing;
@property BOOL videoErrorShown;
@property NSButton *loopButton;
@property PDFView *pdfView;
@property NSTextField *pageField;
@property NSTextField *pageTotal;
@property NSSearchField *searchField;
- (instancetype)initWithURL:(NSURL*)url kind:(NSInteger)kind error:(NSString**)error;
- (void)updateInfo;
@end

@implementation EGMediaController
- (NSButton*)button:(NSString*)title action:(SEL)action {
    NSButton *button=[NSButton buttonWithTitle:title target:self action:action];
    button.bezelStyle=NSBezelStyleTexturedRounded;button.controlSize=NSControlSizeRegular;
    [self.bar addArrangedSubview:button];return button;
}
- (void)spacer {
    NSView *spacer=[[NSView alloc]init];[spacer setContentHuggingPriority:1 forOrientation:NSLayoutConstraintOrientationHorizontal];[self.bar addArrangedSubview:spacer];
}
- (void)mount:(NSView*)view {
    NSView *root=self.window.contentView;view.translatesAutoresizingMaskIntoConstraints=NO;[root addSubview:view];
    [NSLayoutConstraint activateConstraints:@[
        [view.topAnchor constraintEqualToAnchor:self.bar.bottomAnchor],
        [view.bottomAnchor constraintEqualToAnchor:self.info.topAnchor constant:-6],
        [view.leadingAnchor constraintEqualToAnchor:root.leadingAnchor],
        [view.trailingAnchor constraintEqualToAnchor:root.trailingAnchor]]];
}
- (instancetype)initWithURL:(NSURL*)url kind:(NSInteger)kind error:(NSString**)error {
    NSImage *image=nil;PDFDocument *pdf=nil;
    if(kind==1){image=EGLoadImage(url,error);if(!image)return nil;}
    if(kind==3){pdf=EGLoadPDF(url,error);if(!pdf)return nil;}
    NSWindow *window=[[EGMediaWindow alloc]initWithContentRect:NSMakeRect(0,0,1100,780) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];
    self=[super initWithWindow:window];if(!self)return nil;
    self.url=url;self.kind=kind;window.delegate=self;window.releasedWhenClosed=NO;window.contentMinSize=NSMakeSize(900,480);
    window.collectionBehavior=NSWindowCollectionBehaviorFullScreenPrimary;window.representedURL=url;
    window.title=[NSString stringWithFormat:@"%@ — %@",url.lastPathComponent,kind==1?@"Images":kind==2?@"Video player":@"PDF viewer"];
    NSView *root=window.contentView;
    self.bar=[[NSStackView alloc]init];self.bar.orientation=NSUserInterfaceLayoutOrientationHorizontal;self.bar.alignment=NSLayoutAttributeCenterY;self.bar.spacing=8;self.bar.edgeInsets=NSEdgeInsetsMake(8,12,8,12);self.bar.translatesAutoresizingMaskIntoConstraints=NO;
    [root addSubview:self.bar];
    self.info=[NSTextField labelWithString:@""];self.info.font=[NSFont systemFontOfSize:11];self.info.textColor=NSColor.secondaryLabelColor;self.info.translatesAutoresizingMaskIntoConstraints=NO;[root addSubview:self.info];
    [NSLayoutConstraint activateConstraints:@[
        [self.bar.topAnchor constraintEqualToAnchor:root.topAnchor], [self.bar.leadingAnchor constraintEqualToAnchor:root.leadingAnchor], [self.bar.trailingAnchor constraintEqualToAnchor:root.trailingAnchor], [self.bar.heightAnchor constraintEqualToConstant:46],
        [self.info.leadingAnchor constraintEqualToAnchor:root.leadingAnchor constant:12], [self.info.trailingAnchor constraintEqualToAnchor:root.trailingAnchor constant:-12], [self.info.bottomAnchor constraintEqualToAnchor:root.bottomAnchor constant:-6], [self.info.heightAnchor constraintEqualToConstant:18]]];
    if(kind==1){
        self.previousButton=[self button:@"Previous" action:@selector(previousImage:)];self.previousButton.keyEquivalent=@"\uF702";self.previousButton.keyEquivalentModifierMask=0;
        self.nextButton=[self button:@"Next" action:@selector(nextImage:)];self.nextButton.keyEquivalent=@"\uF703";self.nextButton.keyEquivalentModifierMask=0;
        [self addZoomButtons];
        self.imageScroll=[[NSScrollView alloc]init];self.imageScroll.contentView=[[EGCenteringClipView alloc]init];self.imageScroll.hasHorizontalScroller=YES;self.imageScroll.hasVerticalScroller=YES;self.imageScroll.autohidesScrollers=YES;self.imageScroll.allowsMagnification=YES;self.imageScroll.minMagnification=0.02;self.imageScroll.maxMagnification=32;self.imageScroll.backgroundColor=NSColor.controlBackgroundColor;
        self.imageView=[[NSImageView alloc]initWithFrame:NSMakeRect(0,0,image.size.width,image.size.height)];self.imageView.image=image;self.imageView.imageScaling=NSImageScaleNone;self.imageView.animates=YES;self.imageView.allowsCutCopyPaste=YES;self.imageScroll.documentView=self.imageView;[self mount:self.imageScroll];
        [self buildImageList];self.autoFit=YES;
        [[NSNotificationCenter defaultCenter]addObserver:self selector:@selector(imageZoomChanged:) name:NSScrollViewDidEndLiveMagnifyNotification object:self.imageScroll];
    }else if(kind==2){
        NSButton *play=[self button:@"Play / Pause" action:@selector(playPause:)];play.keyEquivalent=@" ";play.keyEquivalentModifierMask=0;
        [self button:@"−10 s" action:@selector(backward:)];[self button:@"+10 s" action:@selector(forward:)];
        self.loopButton=[NSButton checkboxWithTitle:@"Loop" target:self action:nil];[self.bar addArrangedSubview:self.loopButton];
        self.videoView=[[AVPlayerView alloc]init];self.videoView.controlsStyle=AVPlayerViewControlsStyleInline;self.videoView.showsFullScreenToggleButton=YES;self.videoView.showsFrameSteppingButtons=YES;self.videoView.videoGravity=AVLayerVideoGravityResizeAspect;
        self.observedItem=[AVPlayerItem playerItemWithURL:url];self.player=[AVPlayer playerWithPlayerItem:self.observedItem];self.videoView.player=self.player;[self mount:self.videoView];
        self.observing=YES;[self.observedItem addObserver:self forKeyPath:@"status" options:NSKeyValueObservingOptionInitial|NSKeyValueObservingOptionNew context:EGPlayerStatus];
        [[NSNotificationCenter defaultCenter]addObserver:self selector:@selector(playbackEnded:) name:AVPlayerItemDidPlayToEndTimeNotification object:self.observedItem];
    }else{
        self.previousButton=[self button:@"Previous" action:@selector(previousPage:)];self.nextButton=[self button:@"Next" action:@selector(nextPage:)];
        self.pageField=[[NSTextField alloc]init];self.pageField.alignment=NSTextAlignmentCenter;self.pageField.target=self;self.pageField.action=@selector(pageEntered:);[self.pageField.widthAnchor constraintEqualToConstant:42].active=YES;[self.bar addArrangedSubview:self.pageField];
        self.pageTotal=[NSTextField labelWithString:@""];[self.bar addArrangedSubview:self.pageTotal];[self addZoomButtons];
        self.searchField=[[NSSearchField alloc]init];self.searchField.placeholderString=@"Find in PDF";self.searchField.sendsWholeSearchString=YES;self.searchField.target=self;self.searchField.action=@selector(findNext:);[self.searchField.widthAnchor constraintEqualToConstant:160].active=YES;[self.bar addArrangedSubview:self.searchField];
        NSButton *find=[self button:@"Find next" action:@selector(findNext:)];find.keyEquivalent=@"f";find.keyEquivalentModifierMask=NSEventModifierFlagCommand;
        self.pdfView=[[PDFView alloc]init];self.pdfView.document=pdf;self.pdfView.autoScales=YES;self.pdfView.displayMode=kPDFDisplaySinglePageContinuous;self.pdfView.displaysPageBreaks=YES;[self mount:self.pdfView];
        if(pdf.isLocked)[self button:@"Unlock" action:@selector(unlockPDF:)];
        [[NSNotificationCenter defaultCenter]addObserver:self selector:@selector(pdfChanged:) name:PDFViewPageChangedNotification object:self.pdfView];
        [[NSNotificationCenter defaultCenter]addObserver:self selector:@selector(pdfChanged:) name:PDFViewScaleChangedNotification object:self.pdfView];
    }
    [self spacer];[self button:@"Open externally" action:@selector(openExternally:)];[self button:@"Reveal" action:@selector(reveal:)];
    [window center];[window.contentView layoutSubtreeIfNeeded];if(kind==1)[self fit:nil];[self updateInfo];return self;
}
- (void)addZoomButtons {
    NSButton *out=[self button:@"−" action:@selector(zoomOut:)];out.keyEquivalent=@"-";out.keyEquivalentModifierMask=NSEventModifierFlagCommand;
    NSButton *in=[self button:@"+" action:@selector(zoomIn:)];in.keyEquivalent=@"+";in.keyEquivalentModifierMask=NSEventModifierFlagCommand;
    NSButton *fit=[self button:@"Fit" action:@selector(fit:)];fit.keyEquivalent=@"0";fit.keyEquivalentModifierMask=NSEventModifierFlagCommand;
    NSButton *actual=[self button:@"100%" action:@selector(actualSize:)];actual.keyEquivalent=@"1";actual.keyEquivalentModifierMask=NSEventModifierFlagCommand;
}
- (void)buildImageList {
    NSSet *types=[NSSet setWithArray:@[@"png",@"jpg",@"jpeg",@"gif",@"bmp",@"tif",@"tiff",@"heic",@"heif",@"webp",@"ico",@"icns",@"svg"]];
    NSArray *files=[[NSFileManager defaultManager]contentsOfDirectoryAtURL:self.url.URLByDeletingLastPathComponent includingPropertiesForKeys:@[NSURLIsDirectoryKey] options:NSDirectoryEnumerationSkipsHiddenFiles error:nil];
    NSMutableArray *images=[NSMutableArray array];
    for(NSURL *file in files){NSNumber *directory=nil;[file getResourceValue:&directory forKey:NSURLIsDirectoryKey error:nil];if(!directory.boolValue&&[types containsObject:file.pathExtension.lowercaseString])[images addObject:file];}
    if(![images containsObject:self.url])[images addObject:self.url];
    [images sortUsingComparator:^NSComparisonResult(NSURL *a,NSURL *b){return [a.lastPathComponent localizedStandardCompare:b.lastPathComponent];}];
    self.images=images;self.imageIndex=[images indexOfObject:self.url];
}
- (void)showImageAt:(NSUInteger)index {
    if(index>=self.images.count)return;NSString *error=nil;NSImage *image=EGLoadImage(self.images[index],&error);
    self.imageIndex=index;self.url=self.images[index];self.window.representedURL=self.url;self.window.title=[self.url.lastPathComponent stringByAppendingString:@" — Images"];
    if(!image){self.imageView.image=nil;[self updateInfo];self.info.stringValue=error;return;}
    self.imageView.image=image;self.imageView.frame=NSMakeRect(0,0,image.size.width,image.size.height);if(self.autoFit)[self fit:nil];[self updateInfo];
}
- (void)previousImage:(id)sender {if(self.imageIndex>0)[self showImageAt:self.imageIndex-1];}
- (void)nextImage:(id)sender {if(self.imageIndex+1<self.images.count)[self showImageAt:self.imageIndex+1];}
- (void)setImageZoom:(CGFloat)zoom {
    self.autoFit=NO;zoom=MAX(self.imageScroll.minMagnification,MIN(self.imageScroll.maxMagnification,zoom));
    [self.imageScroll setMagnification:zoom centeredAtPoint:NSMakePoint(NSMidX(self.imageView.bounds),NSMidY(self.imageView.bounds))];[self updateInfo];
}
- (void)zoomIn:(id)sender {if(self.kind==1)[self setImageZoom:self.imageScroll.magnification*1.25];else [self.pdfView zoomIn:sender];[self updateInfo];}
- (void)zoomOut:(id)sender {if(self.kind==1)[self setImageZoom:self.imageScroll.magnification/1.25];else [self.pdfView zoomOut:sender];[self updateInfo];}
- (void)actualSize:(id)sender {if(self.kind==1)[self setImageZoom:1./MAX(1.,self.window.backingScaleFactor)];else{self.pdfView.autoScales=NO;self.pdfView.scaleFactor=1;}[self updateInfo];}
- (void)fit:(id)sender {
    if(self.kind==1&&self.imageView.image){NSSize viewport=self.imageScroll.contentSize;NSSize image=self.imageView.image.size;CGFloat zoom=MIN(1./MAX(1.,self.window.backingScaleFactor),MIN(viewport.width/image.width,viewport.height/image.height));[self setImageZoom:MAX(0.02,zoom)];self.autoFit=YES;}
    else if(self.kind==3){self.pdfView.autoScales=YES;}
    [self updateInfo];
}
- (void)imageZoomChanged:(NSNotification*)note {self.autoFit=NO;[self updateInfo];}
- (void)windowDidResize:(NSNotification*)note {if(self.kind==1&&self.autoFit)[self fit:nil];}
- (void)previousPage:(id)sender {[self.pdfView goToPreviousPage:sender];[self updateInfo];}
- (void)nextPage:(id)sender {[self.pdfView goToNextPage:sender];[self updateInfo];}
- (void)pageEntered:(id)sender {
    NSUInteger total=self.pdfView.document.pageCount;NSInteger requested=self.pageField.integerValue;if(!total)return;
    requested=MAX(1,MIN((NSInteger)total,requested));[self.pdfView goToPage:[self.pdfView.document pageAtIndex:requested-1]];[self updateInfo];
}
- (void)pdfChanged:(NSNotification*)note {[self updateInfo];}
- (void)findNext:(id)sender {
    [self.window makeFirstResponder:self.searchField];NSString *query=self.searchField.stringValue;
    if(!query.length){self.pdfView.currentSelection=nil;[self updateInfo];return;}
    PDFDocument *document=self.pdfView.document;
    PDFSelection *selection=[document findString:query fromSelection:self.pdfView.currentSelection withOptions:NSCaseInsensitiveSearch];
    if(!selection)selection=[document findString:query fromSelection:nil withOptions:NSCaseInsensitiveSearch];
    if(selection){[self.pdfView setCurrentSelection:selection animate:YES];[self.pdfView goToSelection:selection];[self updateInfo];}
    else{self.info.stringValue=@"Text not found in this PDF.";}
}
- (void)unlockPDF:(id)sender {
    if(!self.pdfView.document.isLocked)return;
    NSAlert *alert=[[NSAlert alloc]init];alert.messageText=@"Unlock PDF";alert.informativeText=@"Enter the document password.";[alert addButtonWithTitle:@"Unlock"];[alert addButtonWithTitle:@"Cancel"];
    NSSecureTextField *password=[[NSSecureTextField alloc]initWithFrame:NSMakeRect(0,0,280,24)];alert.accessoryView=password;
    [alert beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse response){
        if(response==NSAlertFirstButtonReturn){if([self.pdfView.document unlockWithPassword:password.stringValue]){self.pdfView.autoScales=YES;[self updateInfo];}else{self.info.stringValue=@"Incorrect password. Click Unlock to try again.";}}
    }];
}
- (void)playPause:(id)sender {
    if(self.player.rate>0){[self.player pause];return;}
    double current=CMTimeGetSeconds(self.player.currentTime);double duration=CMTimeGetSeconds(self.player.currentItem.duration);
    if(isfinite(current)&&isfinite(duration)&&duration>0&&current>=duration-0.1){
        __weak EGMediaController *weakSelf=self;[self.player seekToTime:kCMTimeZero completionHandler:^(BOOL done){if(done)dispatch_async(dispatch_get_main_queue(),^{if(weakSelf.window.isVisible)[weakSelf.player play];});}];
    }else{[self.player play];}
}
- (void)seekBy:(double)delta {
    double current=CMTimeGetSeconds(self.player.currentTime);double duration=CMTimeGetSeconds(self.player.currentItem.duration);
    double target=MAX(0,(isfinite(current)?current:0)+delta);if(isfinite(duration)&&duration>0)target=MIN(target,duration);
    [self.player seekToTime:CMTimeMakeWithSeconds(target,600)];
}
- (void)backward:(id)sender {[self seekBy:-10];}
- (void)forward:(id)sender {[self seekBy:10];}
- (void)playbackEnded:(NSNotification*)note {
    if(self.loopButton.state==NSControlStateValueOn){__weak EGMediaController *weakSelf=self;[self.player seekToTime:kCMTimeZero completionHandler:^(BOOL done){if(done)dispatch_async(dispatch_get_main_queue(),^{if(weakSelf.window.isVisible)[weakSelf.player play];});}];}
}
- (void)observeValueForKeyPath:(NSString*)keyPath ofObject:(id)object change:(NSDictionary*)change context:(void*)context {
    if(context!=EGPlayerStatus){[super observeValueForKeyPath:keyPath ofObject:object change:change context:context];return;}
    __weak EGMediaController *weakSelf=self;
    dispatch_async(dispatch_get_main_queue(),^{
        EGMediaController *controller=weakSelf;if(!controller||!controller.observing)return;
        [controller updateInfo];
        if(controller.observedItem.status==AVPlayerItemStatusReadyToPlay&&controller.window.isVisible)[controller.player play];
        else if(controller.observedItem.status==AVPlayerItemStatusFailed&&!controller.videoErrorShown){
            controller.videoErrorShown=YES;NSAlert *alert=[[NSAlert alloc]init];alert.messageText=@"Video could not be played";alert.informativeText=controller.observedItem.error.localizedDescription?:@"This format or codec is not supported by macOS.";[alert addButtonWithTitle:@"OK"];[alert addButtonWithTitle:@"Open externally"];
            if(controller.window.isVisible)[alert beginSheetModalForWindow:controller.window completionHandler:^(NSModalResponse response){if(response==NSAlertSecondButtonReturn)[controller openExternally:nil];}];
        }
    });
}
- (void)updateInfo {
    if(self.kind==1){NSSize dimensions=self.imageView.image.size;self.info.stringValue=[NSString stringWithFormat:@"%.0f × %.0f pixels   ·   %.0f%%   ·   %lu of %lu",dimensions.width,dimensions.height,self.imageScroll.magnification*MAX(1.,self.window.backingScaleFactor)*100,(unsigned long)self.imageIndex+1,(unsigned long)self.images.count];self.previousButton.enabled=self.imageIndex>0;self.nextButton.enabled=self.imageIndex+1<self.images.count;}
    else if(self.kind==2){self.info.stringValue=self.observedItem.status==AVPlayerItemStatusFailed?(self.observedItem.error.localizedDescription?:@"Playback failed"):self.observedItem.status==AVPlayerItemStatusUnknown?@"Loading video…":@"Space: play/pause   ·   Native playback controls: seek, volume, frame stepping and full screen";}
    else{PDFDocument *document=self.pdfView.document;NSUInteger index=self.pdfView.currentPage?[document indexForPage:self.pdfView.currentPage]:NSNotFound;self.pageField.stringValue=index==NSNotFound?@"":[NSString stringWithFormat:@"%lu",(unsigned long)index+1];self.pageTotal.stringValue=[NSString stringWithFormat:@"of %lu",(unsigned long)document.pageCount];self.previousButton.enabled=self.pdfView.canGoToPreviousPage;self.nextButton.enabled=self.pdfView.canGoToNextPage;self.info.stringValue=document.isLocked?@"Password-protected PDF. Click Unlock to enter its password.":[NSString stringWithFormat:@"%lu pages   ·   %.0f%%   ·   Select text to copy",(unsigned long)document.pageCount,self.pdfView.scaleFactor*100];}
}
- (void)openExternally:(id)sender {[[NSWorkspace sharedWorkspace]openURL:self.url];}
- (void)reveal:(id)sender {[[NSWorkspace sharedWorkspace]activateFileViewerSelectingURLs:@[self.url]];}
- (void)windowWillClose:(NSNotification*)note {
    [self.player pause];self.videoView.player=nil;
    if(self.observing){[self.observedItem removeObserver:self forKeyPath:@"status" context:EGPlayerStatus];self.observing=NO;}
    [[NSNotificationCenter defaultCenter]removeObserver:self];self.pdfView.document=nil;self.imageView.image=nil;
    dispatch_async(dispatch_get_main_queue(),^{
        [EGViewers removeObject:self];
        if(EGViewers.count==0&&EGMainWindowCount==0)[NSApp terminate:nil];
    });
}
@end

size_t explorer_viewer_count(void) {return EGViewers.count;}
int explorer_open_media(const char *path,int kind,char **error) {
    @autoreleasepool { @try {
        if(![NSThread isMainThread]){*error=strdup("Media windows must be opened on the main thread.");return 0;}
        if(kind<1||kind>3){*error=strdup("Unsupported viewer kind.");return 0;}
        NSURL *url=EGFileURL(path);
        if(!EGViewers)EGViewers=[NSMutableSet set];
        for(EGMediaController *existing in EGViewers)if(existing.kind==kind&&[existing.url isEqual:url]){[existing.window deminiaturize:nil];[existing.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];return 1;}
        NSString *failure=nil;EGMediaController *viewer=[[EGMediaController alloc]initWithURL:url kind:kind error:&failure];
        if(!viewer){*error=strdup((failure?:@"Could not create the media viewer.").UTF8String);return 0;}
        [EGViewers addObject:viewer];[viewer showWindow:nil];[viewer.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];return 1;
    } @catch(NSException *exception){*error=strdup((exception.reason?:@"Media viewer error").UTF8String);return 0;} }
}
