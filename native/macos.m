#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#include <stdlib.h>
#include <string.h>

static char *copy_string(NSString *value) { return strdup(value.UTF8String ?: ""); }
// Returned strings are malloc-owned. The Rust boundary always calls explorer_free.
void explorer_free(char *value) { free(value); }
double explorer_double_click_interval(void) { return NSEvent.doubleClickInterval; }
char *explorer_trash(const char *path, char **error) {
    @autoreleasepool {
        NSString *name = [[NSFileManager defaultManager] stringWithFileSystemRepresentation:path length:strlen(path)];
        NSURL *result = nil; NSError *failure = nil;
        if (![[NSFileManager defaultManager] trashItemAtURL:[NSURL fileURLWithPath:name] resultingItemURL:&result error:&failure]) {
            *error = copy_string(failure.localizedDescription); return NULL;
        }
        if (!result) { *error=copy_string(@"Trash did not return the resulting item URL."); return NULL; }
        return strdup(result.fileSystemRepresentation);
    }
}
long explorer_clipboard_write(const char **paths, size_t count) {
    @autoreleasepool {
        NSMutableArray *urls = [NSMutableArray array];
        for (size_t i=0;i<count;i++) {
            NSString *p = [[NSFileManager defaultManager] stringWithFileSystemRepresentation:paths[i] length:strlen(paths[i])];
            [urls addObject:[NSURL fileURLWithPath:p]];
        }
        NSPasteboard *board = NSPasteboard.generalPasteboard;
        [board clearContents]; [board writeObjects:urls]; return board.changeCount;
    }
}
long explorer_clipboard_version(void) { return NSPasteboard.generalPasteboard.changeCount; }
char *explorer_clipboard_read(void) {
    @autoreleasepool {
        NSArray *urls = [NSPasteboard.generalPasteboard readObjectsForClasses:@[NSURL.class] options:@{NSPasteboardURLReadingFileURLsOnlyKey:@YES}];
        NSMutableString *out = [NSMutableString string];
        for (NSURL *url in urls) {
            const unsigned char *bytes = (const unsigned char *)url.fileSystemRepresentation;
            for (size_t i=0;i<strlen((const char *)bytes);i++) [out appendFormat:@"%02x",bytes[i]];
            [out appendString:@"\n"];
        }
        return copy_string(out);
    }
}
