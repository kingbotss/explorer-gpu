#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#include <arpa/inet.h>
int main(int argc,const char *argv[]) {
    @autoreleasepool {
        if(argc!=2)return 1;
        NSString *folder=[NSString stringWithUTF8String:argv[1]];
        [[NSFileManager defaultManager] createDirectoryAtPath:folder withIntermediateDirectories:YES attributes:nil error:nil];
        NSMutableData *icns=[NSMutableData dataWithBytes:"icns\0\0\0\0" length:8];
        NSDictionary *types=@{@16:@[@"icp4",@"ic11"],@32:@[@"icp5",@"ic12"],@128:@[@"ic07",@"ic13"],@256:@[@"ic08",@"ic14"],@512:@[@"ic09",@"ic10"]};
        for(NSNumber *n in @[@16,@32,@128,@256,@512]) {
            int base=n.intValue;
            for(int scale=1;scale<=2;scale++) {
                int side=base*scale;
                NSBitmapImageRep *rep=[[NSBitmapImageRep alloc] initWithBitmapDataPlanes:NULL pixelsWide:side pixelsHigh:side bitsPerSample:8 samplesPerPixel:4 hasAlpha:YES isPlanar:NO colorSpaceName:NSDeviceRGBColorSpace bytesPerRow:0 bitsPerPixel:0];
                NSGraphicsContext *context=[NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
                [NSGraphicsContext saveGraphicsState];[NSGraphicsContext setCurrentContext:context];
                NSAffineTransform *transform=[NSAffineTransform transform];[transform scaleBy:side/1024.0];[transform concat];
                NSBezierPath *tile=[NSBezierPath bezierPathWithRoundedRect:NSMakeRect(80,80,864,864) xRadius:194 yRadius:194];
                NSGradient *gradient=[[NSGradient alloc] initWithStartingColor:[NSColor colorWithRed:0.97 green:0.98 blue:1.0 alpha:1] endingColor:[NSColor colorWithRed:0.86 green:0.90 blue:0.96 alpha:1]];
                [gradient drawInBezierPath:tile angle:-90];
                NSBezierPath *back=[NSBezierPath bezierPath];
                [back moveToPoint:NSMakePoint(225,350)];[back lineToPoint:NSMakePoint(225,694)];[back curveToPoint:NSMakePoint(255,724) controlPoint1:NSMakePoint(225,716) controlPoint2:NSMakePoint(234,724)];
                [back lineToPoint:NSMakePoint(431,724)];[back lineToPoint:NSMakePoint(493,660)];[back lineToPoint:NSMakePoint(769,660)];[back lineToPoint:NSMakePoint(799,350)];[back closePath];
                [[NSColor colorWithRed:0.99 green:0.84 blue:0.39 alpha:1] setFill];[back fill];
                NSBezierPath *front=[NSBezierPath bezierPathWithRoundedRect:NSMakeRect(225,302,574,308) xRadius:30 yRadius:30];
                [[NSColor colorWithRed:0.96 green:0.76 blue:0.26 alpha:1] setFill];[front fill];
                [[NSColor colorWithRed:0.22 green:0.46 blue:0.88 alpha:1] setFill];
                for(int row=0;row<2;row++)for(int col=0;col<2;col++)[[NSBezierPath bezierPathWithRoundedRect:NSMakeRect(456+col*64,394+row*64,48,48) xRadius:6 yRadius:6] fill];
                [NSGraphicsContext restoreGraphicsState];
                NSString *name=[NSString stringWithFormat:@"icon_%dx%d%@.png",base,base,scale==2?@"@2x":@""];
                NSData *png=[rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
                if(![png writeToFile:[folder stringByAppendingPathComponent:name] atomically:YES])return 2;
                NSString *type=types[n][scale-1]; uint32_t length=htonl((uint32_t)png.length+8);
                [icns appendBytes:type.UTF8String length:4];[icns appendBytes:&length length:4];[icns appendData:png];
            }
        }
        uint32_t total=htonl((uint32_t)icns.length);[icns replaceBytesInRange:NSMakeRange(4,4) withBytes:&total];
        if(![icns writeToFile:[folder stringByAppendingString:@".icns"] atomically:YES])return 3;
    }return 0;
}
