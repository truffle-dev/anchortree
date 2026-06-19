//! Visual Set-of-Mark escalation (feature `visual-marks`, opt-in).
//!
//! anchortree's default handle surface is *textual*: the durable [`Diff`] plus a
//! short list of transient [`Mark`]s, each rendered as one line
//! (`m3 btn "Add to cart" @312,48`). That is an order of magnitude cheaper in
//! tokens than a screenshot, and it is the canonical path (`DECISIONS.md` D13).
//!
//! But a mark is only as good as the anchor underneath it, and there is one case
//! the text path cannot serve: an interactive region that the engine can *see*
//! the geometry of but cannot give a DOM/AX node for — a hit area painted into a
//! `<canvas>`, a WebGL scene, a plugin `<embed>`. There is no `backendNodeId` to
//! act on and no role/label to print, so no text mark gets minted at all.
//!
//! For that case this module draws the *visual* form of set-of-mark prompting: a
//! page screenshot with a numbered box over each mark, aligned to the exact same
//! [`Mark::geometry`] the text path uses. A vision-capable agent reads the
//! numbered overlay, picks a box, and acts on the corresponding mark index. The
//! escalation is **opt-in by construction** — the feature is off by default, the
//! optional `image` dependency only enters the build when it is asked for, and
//! the textual path is untouched.
//!
//! ## What this delivers, and what it does not
//!
//! The library half is *deterministic*: given a PNG and a set of marks with
//! geometry, [`render_marked_png`] produces a numbered overlay with no model in
//! the loop, and every box is pixel-aligned to a mark's box. That is the piece a
//! caller cannot easily write themselves (CDP capture → decode → aligned overlay
//! → re-encode) and the piece this crate can test hermetically.
//!
//! What it does **not** do is *segment* a DOM-less surface into the marks in the
//! first place. Turning a raw `<canvas>` into "here are the three clickable
//! regions" is a vision-model judgment, not a protocol operation, and it stays
//! out of this crate (`DECISIONS.md` D56). [`screenshot_with_marks`] overlays
//! the marks it is handed; producing those marks for a truly anchorless surface
//! is the agent's job, above this seam.
//!
//! ## Coordinate space and device pixel ratio
//!
//! [`Mark::geometry`] is in CSS pixels (the same space the text path prints
//! `@x,y` in). `Page.captureScreenshot` returns a raster whose pixels are CSS
//! pixels times the page's device-pixel-ratio. When the DPR is not 1, pass it as
//! [`MarkOverlay::scale`] so the boxes land on the right pixels; the default of
//! `1.0` is correct for the common headless case (DPR 1). Boxes are clamped to
//! the image bounds, so a partly-offscreen mark draws the visible part rather
//! than panicking.
//!
//! [`Diff`]: anchortree_core::Diff
//! [`Mark`]: anchortree_core::Mark
//! [`Mark::geometry`]: anchortree_core::Mark::geometry

use anchortree_core::Mark;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams,
};
use image::{ImageFormat, Rgba, RgbaImage};

use crate::channel::CdpChannel;
use crate::error::CdpError;

/// An error producing a marked screenshot.
#[derive(Debug, thiserror::Error)]
pub enum VisualError {
    /// A CDP command (the screenshot capture) failed.
    #[error("cdp error capturing screenshot: {0}")]
    Cdp(#[from] CdpError),
    /// The base64 the screenshot rode in on did not decode.
    #[error("screenshot was not valid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    /// The captured (or supplied) bytes were not a PNG we could decode, or the
    /// overlaid image could not be re-encoded.
    #[error("png codec error: {0}")]
    Image(#[from] image::ImageError),
}

/// How to draw the numbered overlay.
///
/// The defaults ([`MarkOverlay::default`]) target a headless capture at
/// device-pixel-ratio 1: scale `1.0`, a 2-pixel box stroke, and a label glyph
/// scale of 2 (each font pixel becomes a 2×2 block, so a digit is 10×14).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkOverlay {
    /// Multiplier from CSS pixels (the space [`Mark::geometry`] lives in) to
    /// raster pixels (the screenshot). Set this to the page's device-pixel-ratio
    /// when it is not 1. Defaults to `1.0`.
    pub scale: f32,
    /// Box outline thickness in raster pixels. Defaults to `2`.
    pub stroke: u32,
    /// Integer magnification of the 5×7 bitmap digits in the label badge.
    /// Defaults to `2`.
    pub glyph_scale: u32,
}

impl Default for MarkOverlay {
    fn default() -> Self {
        Self {
            scale: 1.0,
            stroke: 2,
            glyph_scale: 2,
        }
    }
}

/// The overlay accent: a strong magenta that reads against light and dark pages
/// alike. Boxes and label badges are drawn in this; digits are drawn white on
/// top of the badge.
const ACCENT: Rgba<u8> = Rgba([255, 0, 102, 255]);
/// The label digits, drawn on top of an [`ACCENT`] badge.
const GLYPH_INK: Rgba<u8> = Rgba([255, 255, 255, 255]);

/// 5×7 bitmap glyphs for the digits `0`–`9`. Each row's low five bits are the
/// pixel mask, most-significant bit leftmost. Hand-rolled so the overlay needs
/// no font crate (consistent with the lean-dependency choices elsewhere — the
/// tokenizer in `budget` is hand-rolled for the same reason).
const DIGITS: [[u8; 7]; 10] = [
    [
        0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
    ], // 0
    [
        0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ], // 1
    [
        0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
    ], // 2
    [
        0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
    ], // 3
    [
        0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
    ], // 4
    [
        0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
    ], // 5
    [
        0b01110, 0b10001, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
    ], // 6
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
    ], // 7
    [
        0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
    ], // 8
    [
        0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b10001, 0b01110,
    ], // 9
];

/// Glyph cell dimensions, before [`MarkOverlay::glyph_scale`].
const GLYPH_W: u32 = 5;
const GLYPH_H: u32 = 7;
/// Pixels of padding inside a label badge, and between adjacent glyphs, before
/// `glyph_scale`.
const GLYPH_GAP: u32 = 1;
const BADGE_PAD: u32 = 2;

/// Capture a PNG screenshot of the page behind `channel` and overlay a numbered
/// box on each of `marks`, returning the PNG bytes of the composite.
///
/// This is the thin live wrapper: it issues one `Page.captureScreenshot`
/// (`format: png`), base64-decodes the reply, and hands the bytes to the pure
/// [`render_marked_png`]. It is generic over [`CdpChannel`], so it drives a
/// locally launched page and a hosted [`RawCdpSession`](crate::RawCdpSession)
/// through the same code.
///
/// The `marks` are typically [`Observation::marks`](anchortree_core::Observation)
/// from the same turn, so the overlay lines up with the textual mark list an
/// agent already has.
pub async fn screenshot_with_marks<C: CdpChannel>(
    channel: &C,
    marks: &[Mark],
    opts: MarkOverlay,
) -> Result<Vec<u8>, VisualError> {
    let shot = channel
        .run(
            CaptureScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .build(),
        )
        .await?;
    // `Binary` is the base64 text of the PNG, not the raw bytes; decode it.
    let png = BASE64.decode(String::from(shot.data))?;
    render_marked_png(&png, marks, opts)
}

/// Decode `png`, draw a numbered box over each mark, and re-encode to PNG.
///
/// Pure and browser-free: this is where all the drawing rigor lives, so it can
/// be tested against a synthesized image with no CDP in the loop. The boxes are
/// aligned to [`Mark::geometry`] scaled by [`MarkOverlay::scale`] and clamped to
/// the image, and each box carries its mark's [`index`](Mark::index) as a label
/// badge in the top-left corner.
pub fn render_marked_png(
    png: &[u8],
    marks: &[Mark],
    opts: MarkOverlay,
) -> Result<Vec<u8>, VisualError> {
    let mut img = image::load_from_memory_with_format(png, ImageFormat::Png)?.to_rgba8();
    draw_marks(&mut img, marks, opts);
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)?;
    Ok(out)
}

/// Draw the numbered overlay onto `img` in place.
///
/// Split out from [`render_marked_png`] so the drawing is testable directly on
/// an [`RgbaImage`] without a PNG round-trip. Everything is clamped to the image
/// bounds: a mark whose box falls partly (or wholly) outside the raster simply
/// draws less, never panics.
fn draw_marks(img: &mut RgbaImage, marks: &[Mark], opts: MarkOverlay) {
    let scale = if opts.scale.is_finite() && opts.scale > 0.0 {
        opts.scale
    } else {
        1.0
    };
    let stroke = opts.stroke.max(1);
    let gscale = opts.glyph_scale.max(1);

    for mark in marks {
        let g = &mark.geometry;
        let x0 = (g.x * scale).floor().max(0.0) as u32;
        let y0 = (g.y * scale).floor().max(0.0) as u32;
        let x1 = ((g.x + g.w) * scale).ceil().max(0.0) as u32;
        let y1 = ((g.y + g.h) * scale).ceil().max(0.0) as u32;
        stroke_rect(img, x0, y0, x1, y1, stroke, ACCENT);
        draw_label(img, x0, y0, mark.index, gscale);
    }
}

/// Stroke the outline of the rectangle `[x0,x1) × [y0,y1)` with a border
/// `thickness` pixels wide, drawn *inside* the rectangle so the badge in the
/// corner always sits on painted pixels. Fully clamped to the image.
fn stroke_rect(
    img: &mut RgbaImage,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    thickness: u32,
    color: Rgba<u8>,
) {
    let (w, h) = img.dimensions();
    let x1 = x1.min(w);
    let y1 = y1.min(h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let t = thickness.min(x1 - x0).min(y1 - y0);
    for y in y0..y1 {
        for x in x0..x1 {
            let on_border = x < x0 + t || x >= x1 - t || y < y0 + t || y >= y1 - t;
            if on_border {
                img.put_pixel(x, y, color);
            }
        }
    }
}

/// Draw the decimal `index` as a white-on-accent badge anchored at the top-left
/// corner `(x, y)` of a box. The badge is sized to the glyphs it holds and
/// clamped to the image.
fn draw_label(img: &mut RgbaImage, x: u32, y: u32, index: usize, gscale: u32) {
    let digits = digits_of(index);
    let glyph_w = GLYPH_W * gscale;
    let glyph_h = GLYPH_H * gscale;
    let gap = GLYPH_GAP * gscale;
    let pad = BADGE_PAD * gscale;

    let n = digits.len() as u32;
    let badge_w = pad * 2 + n * glyph_w + n.saturating_sub(1) * gap;
    let badge_h = pad * 2 + glyph_h;

    fill_rect(img, x, y, x + badge_w, y + badge_h, ACCENT);

    let mut gx = x + pad;
    let gy = y + pad;
    for d in digits {
        blit_glyph(img, gx, gy, &DIGITS[d as usize], gscale);
        gx += glyph_w + gap;
    }
}

/// Fill the rectangle `[x0,x1) × [y0,y1)` with `color`, clamped to the image.
fn fill_rect(img: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, color: Rgba<u8>) {
    let (w, h) = img.dimensions();
    let x1 = x1.min(w);
    let y1 = y1.min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            img.put_pixel(x, y, color);
        }
    }
}

/// Blit one 5×7 glyph magnified `gscale`× at `(x, y)`, in [`GLYPH_INK`],
/// clamped to the image.
fn blit_glyph(img: &mut RgbaImage, x: u32, y: u32, glyph: &[u8; 7], gscale: u32) {
    let (w, h) = img.dimensions();
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..GLYPH_W {
            // Bit (GLYPH_W-1-col) is the leftmost pixel of the row.
            if bits & (1 << (GLYPH_W - 1 - col)) != 0 {
                let px0 = x + col * gscale;
                let py0 = y + row as u32 * gscale;
                for dy in 0..gscale {
                    for dx in 0..gscale {
                        let px = px0 + dx;
                        let py = py0 + dy;
                        if px < w && py < h {
                            img.put_pixel(px, py, GLYPH_INK);
                        }
                    }
                }
            }
        }
    }
}

/// The decimal digits of `index`, most significant first (so `0` → `[0]`,
/// `42` → `[4, 2]`).
fn digits_of(index: usize) -> Vec<u8> {
    if index == 0 {
        return vec![0];
    }
    let mut ds = Vec::new();
    let mut n = index;
    while n > 0 {
        ds.push((n % 10) as u8);
        n /= 10;
    }
    ds.reverse();
    ds
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchortree_core::{Bbox, Role};

    fn mark(index: usize, x: f32, y: f32, w: f32, h: f32) -> Mark {
        // `Mark::from_parts` is crate-private to anchortree-core; its fields are
        // public, so a struct literal is the cross-crate way to build one.
        Mark {
            index,
            backend_node_id: 1,
            role: Role::Button,
            label_snippet: "x".to_string(),
            geometry: Bbox { x, y, w, h },
        }
    }

    fn blank(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]))
    }

    #[test]
    fn digits_of_handles_zero_and_multidigit() {
        assert_eq!(digits_of(0), vec![0]);
        assert_eq!(digits_of(7), vec![7]);
        assert_eq!(digits_of(42), vec![4, 2]);
        assert_eq!(digits_of(105), vec![1, 0, 5]);
    }

    #[test]
    fn box_border_is_painted_in_accent_interior_is_not() {
        let mut img = blank(60, 60);
        // A 20×20 box at (10,10) with a thin 1px stroke and tiny glyphs so the
        // badge does not cover the far corner we probe.
        let opts = MarkOverlay {
            scale: 1.0,
            stroke: 1,
            glyph_scale: 1,
        };
        draw_marks(&mut img, &[mark(0, 10.0, 10.0, 20.0, 20.0)], opts);

        // Top-left corner pixel of the box is on the border.
        assert_eq!(*img.get_pixel(10, 10), ACCENT);
        // Bottom-right border pixel (x1-1, y1-1) = (29, 29).
        assert_eq!(*img.get_pixel(29, 29), ACCENT);
        // A pixel well inside the box, away from the badge, stays background.
        assert_eq!(*img.get_pixel(25, 25), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn scale_moves_the_box_into_raster_pixels() {
        let mut img = blank(100, 100);
        let opts = MarkOverlay {
            scale: 2.0,
            stroke: 1,
            glyph_scale: 1,
        };
        // CSS box at (10,10); at scale 2 its top-left lands at raster (20,20).
        draw_marks(&mut img, &[mark(0, 10.0, 10.0, 10.0, 10.0)], opts);
        assert_eq!(*img.get_pixel(20, 20), ACCENT);
        // The unscaled position is background.
        assert_eq!(*img.get_pixel(10, 10), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn label_badge_paints_ink_over_accent() {
        let mut img = blank(80, 80);
        // Mark index 8 (a glyph with a filled top row) at the origin corner.
        draw_marks(
            &mut img,
            &[mark(8, 0.0, 0.0, 40.0, 40.0)],
            MarkOverlay::default(),
        );
        // Somewhere in the badge there must be both accent (badge fill) and
        // white ink (the digit). Scan the badge region.
        let mut saw_accent = false;
        let mut saw_ink = false;
        for y in 0..20 {
            for x in 0..20 {
                let p = *img.get_pixel(x, y);
                if p == ACCENT {
                    saw_accent = true;
                }
                if p == GLYPH_INK {
                    saw_ink = true;
                }
            }
        }
        assert!(saw_accent, "badge fill should paint accent");
        assert!(saw_ink, "digit should paint white ink on the badge");
    }

    #[test]
    fn offscreen_mark_is_clamped_not_panicking() {
        let mut img = blank(40, 40);
        // A box that starts well outside the image: must not panic, and must
        // leave at least the far interior untouched.
        draw_marks(
            &mut img,
            &[mark(3, 100.0, 100.0, 50.0, 50.0)],
            MarkOverlay::default(),
        );
        assert_eq!(*img.get_pixel(0, 0), Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn render_marked_png_round_trips_to_a_valid_png() {
        // Encode a blank PNG, run the full decode→draw→encode path, and confirm
        // the output decodes again and carries the overlay.
        let mut src = Vec::new();
        blank(50, 50)
            .write_to(&mut std::io::Cursor::new(&mut src), ImageFormat::Png)
            .unwrap();

        let out = render_marked_png(
            &src,
            &[mark(0, 5.0, 5.0, 30.0, 30.0)],
            MarkOverlay {
                scale: 1.0,
                stroke: 2,
                glyph_scale: 1,
            },
        )
        .unwrap();

        let decoded = image::load_from_memory_with_format(&out, ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (50, 50));
        // The box border at the top-left corner is accent in the re-decoded PNG.
        assert_eq!(*decoded.get_pixel(5, 5), ACCENT);
    }

    #[test]
    fn render_marked_png_rejects_non_png_bytes() {
        let err = render_marked_png(b"not a png", &[], MarkOverlay::default());
        assert!(matches!(err, Err(VisualError::Image(_))));
    }

    #[test]
    fn multiple_marks_each_get_their_own_box() {
        let mut img = blank(120, 60);
        draw_marks(
            &mut img,
            &[
                mark(0, 5.0, 5.0, 30.0, 30.0),
                mark(1, 70.0, 5.0, 30.0, 30.0),
            ],
            MarkOverlay {
                scale: 1.0,
                stroke: 1,
                glyph_scale: 1,
            },
        );
        // Each box's top-left corner is painted.
        assert_eq!(*img.get_pixel(5, 5), ACCENT);
        assert_eq!(*img.get_pixel(70, 5), ACCENT);
        // The gap between them is untouched.
        assert_eq!(*img.get_pixel(50, 30), Rgba([0, 0, 0, 255]));
    }
}
