use gpui::{prelude::*, *};
use std::borrow::Cow;
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let name = path
            .strip_prefix("icons/")
            .unwrap_or(path)
            .trim_end_matches(".svg");
        let shape = match name {
            "folder" => {
                "<path d='M3 6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v10H3Z'/><path d='M3 10h18'/>"
            }
            "home" => "<path d='m3 10 9-7 9 7v11h-7v-7h-4v7H3Z'/>",
            "download" => "<path d='M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5'/>",
            "desktop" => {
                "<rect x='3' y='3' width='18' height='14' rx='2'/><path d='M8 21h8m-4-4v4'/>"
            }
            "file" => "<path d='M5 3h9l5 5v13H5Z'/><path d='M14 3v6h5M8 13h8m-8 4h6'/>",
            "image" => {
                "<rect x='3' y='3' width='18' height='18' rx='2'/><circle cx='8' cy='8' r='1.5'/><path d='m3 17 5-5 4 4 4-6 5 7'/>"
            }
            "video" => {
                "<rect x='3' y='5' width='13' height='14' rx='2'/><path d='m16 10 5-4v12l-5-4'/>"
            }
            "music" => {
                "<path d='M9 18V5l12-2v13M9 9l12-2'/><ellipse cx='6' cy='18' rx='3' ry='3'/><ellipse cx='18' cy='16' rx='3' ry='3'/>"
            }
            "archive" => {
                "<rect x='4' y='3' width='16' height='18' rx='2'/><path d='M10 3v11h4V3m-4 4h4m-4 4h4'/><rect x='10' y='16' width='4' height='3'/>"
            }
            "code" => "<path d='m8 6-6 6 6 6m8-12 6 6-6 6m-3-15-2 18'/>",
            "link" => {
                "<path d='m10 14 4-4m-7 1-3 3a4 4 0 0 0 6 6l3-3m-2-10 3-3a4 4 0 0 1 6 6l-3 3'/>"
            }
            "drive" => "<path d='m3 14 3-9h12l3 9v6H3Z'/><path d='M3 14h18m-4 3h1'/>",
            "cloud" => "<path d='M6 19a5 5 0 0 1-1-10 7 7 0 0 1 13-1 5.5 5.5 0 0 1 0 11Z'/>",
            "star" => "<path d='m12 2 3 6 7 1-5 5 1 7-6-3-6 3 1-7-5-5 7-1Z'/>",
            "pin" => "<path d='m9 3 12 12-5 1-4 4-3-5-5-3 4-4Zm-1 13-6 6'/>",
            "trash" => "<path d='M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7'/>",
            "search" => "<circle cx='10' cy='10' r='7'/><path d='m15 15 7 7'/>",
            "back" => "<path d='m14 5-7 7 7 7'/>",
            "forward" => "<path d='m10 5 7 7-7 7'/>",
            "up" => "<path d='m5 12 7-7 7 7m-7-7v17'/>",
            "down" => "<path d='m5 9 7 7 7-7'/>",
            "refresh" => "<path d='M20 8a9 9 0 1 0 0 9M20 3v6h-6'/>",
            "sort" => "<path d='M4 5h16M4 12h11M4 19h6'/>",
            "plus" => "<path d='M12 4v16M4 12h16'/>",
            "close" => "<path d='m6 6 12 12M6 18 18 6'/>",
            "copy" => "<rect x='8' y='8' width='13' height='13' rx='2'/><path d='M16 8V3H3v13h5'/>",
            "cut" => {
                "<circle cx='6' cy='6' r='3'/><circle cx='6' cy='18' r='3'/><path d='m9 7 12 13M9 17 21 4'/>"
            }
            "paste" => {
                "<path d='M9 5H4v16h16V5h-5'/><rect x='9' y='2' width='6' height='6' rx='1'/>"
            }
            "rename" => "<path d='m4 16 12-12 4 4L8 20H4ZM14 6l4 4'/>",
            "list" => "<path d='M8 5h14M8 12h14M8 19h14M2 5h1m-1 7h1m-1 7h1'/>",
            "grid" => {
                "<rect x='3' y='3' width='7' height='7' rx='1'/><rect x='14' y='3' width='7' height='7' rx='1'/><rect x='3' y='14' width='7' height='7' rx='1'/><rect x='14' y='14' width='7' height='7' rx='1'/>"
            }
            "preview" => "<rect x='3' y='4' width='18' height='16' rx='2'/><path d='M14 4v16'/>",
            "eye" => {
                "<path d='M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12Z'/><circle cx='12' cy='12' r='3'/>"
            }
            "more" => {
                "<circle cx='5' cy='12' r='1'/><circle cx='12' cy='12' r='1'/><circle cx='19' cy='12' r='1'/>"
            }
            "info" => "<circle cx='12' cy='12' r='9'/><path d='M12 11v6m0-11v1'/>",
            "terminal" => {
                "<rect x='2' y='4' width='20' height='16' rx='2'/><path d='m6 8 4 4-4 4m7 0h5'/>"
            }
            "undo" => "<path d='M3 4v7h7M3 11a9 9 0 1 1 2 8'/>",
            "sun" => {
                "<circle cx='12' cy='12' r='4'/><path d='M12 1v3m0 16v3M1 12h3m16 0h3M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2'/>"
            }
            "check" => "<path d='m4 12 5 5L20 6'/>",
            "help" => {
                "<circle cx='12' cy='12' r='9'/><path d='M9 8a3 3 0 1 1 4 3c-1 1-1 1-1 3m0 3v1'/>"
            }
            _ => return Ok(None),
        };
        Ok(Some(Cow::Owned(format!("<svg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24' fill='none' stroke='black' stroke-width='1.65' stroke-linecap='round' stroke-linejoin='round'>{shape}</svg>").into_bytes())))
    }
    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
pub fn icon(name: &str, color: u32, size: f32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .text_color(rgb(color))
        .size(px(size))
        .flex_shrink_0()
}

/// Code-native, two-tone folder glyph matching Explorer's yellow folder treatment.
pub fn item_icon(name: &str, color: u32, side: f32) -> AnyElement {
    if name != "folder" {
        return icon(name, color, side).into_any_element();
    }
    div()
        .relative()
        .size(px(side))
        .flex_shrink_0()
        .child(
            div()
                .absolute()
                .left(px(side * 0.04))
                .top(px(side * 0.16))
                .w(px(side * 0.45))
                .h(px(side * 0.24))
                .rounded_t_sm()
                .bg(rgb(0xf7cf66)),
        )
        .child(
            div()
                .absolute()
                .left(px(side * 0.04))
                .top(px(side * 0.29))
                .w(px(side * 0.92))
                .h(px(side * 0.54))
                .rounded_sm()
                .bg(rgb(0xffdf82)),
        )
        .child(
            div()
                .absolute()
                .left(px(side * 0.04))
                .top(px(side * 0.39))
                .w(px(side * 0.92))
                .h(px(side * 0.45))
                .rounded_sm()
                .bg(rgb(0xf3c44c)),
        )
        .child(
            div()
                .absolute()
                .left(px(side * 0.08))
                .top(px(side * 0.41))
                .w(px(side * 0.84))
                .h(px(side * 0.025))
                .bg(rgb(0xffe7a5)),
        )
        .into_any_element()
}
