//! Saving an on-page `<svg>` (the network map) as its own standalone file: as SVG itself, or
//! rasterized to PNG. The live element only renders correctly because of this page's own
//! stylesheet (CSS classes, design-token custom properties) and its parent document — neither of
//! which a saved file still has, so a plain copy of its markup would come out as bare black
//! shapes with no colors or line styles. [`standalone_markup`] fixes that once, up front, by
//! asking the browser for each element's own already-resolved style (`getComputedStyle`, the
//! same value that stylesheet produced) and writing it back onto a clone as a plain inline
//! `style`, so the result means the same thing with nothing else around it.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, CanvasRenderingContext2d, Element, HtmlAnchorElement, HtmlCanvasElement,
    HtmlImageElement, Url,
};
use yew::NodeRef;

/// The presentation properties an exported map actually uses; copied as resolved, literal values
/// from [`web_sys::Window::get_computed_style`] onto every element of the clone, so the result
/// no longer depends on this page's stylesheet or design-token variables (`local.css`'s
/// `.map-*` rules, `var(--pf-t--...)`) to look the same.
const PROPERTIES: &[&str] = &[
    "fill",
    "stroke",
    "stroke-width",
    "stroke-dasharray",
    "font-size",
    "font-family",
    "text-anchor",
];

/// `svg_ref`'s live element, deep-cloned with every descendant's relevant style resolved and
/// inlined (see the module doc comment), its own background painted in behind everything else
/// (a saved file has no page behind it to show through), and `xmlns` set so it's a valid
/// standalone SVG document, not just a fragment that only parses inside an HTML page. The width
/// and height that follow are parsed back out of the live element's own `viewBox`, for the PNG
/// path's canvas.
fn standalone_markup(svg_ref: &NodeRef) -> Option<(String, f64, f64)> {
    let svg_el = svg_ref.cast::<Element>()?;
    let window = web_sys::window()?;
    let clone: Element = svg_el.clone_node_with_deep(true).ok()?.dyn_into().ok()?;
    clone
        .set_attribute("xmlns", "http://www.w3.org/2000/svg")
        .ok()?;

    let live_all = svg_el.query_selector_all("*").ok()?;
    let clone_all = clone.query_selector_all("*").ok()?;
    for i in 0..live_all.length() {
        let (Some(live_node), Some(clone_node)) = (live_all.get(i), clone_all.get(i)) else {
            continue;
        };
        let (Ok(live_el), Ok(clone_el)) = (
            live_node.dyn_into::<Element>(),
            clone_node.dyn_into::<Element>(),
        ) else {
            continue;
        };
        let Ok(Some(computed)) = window.get_computed_style(&live_el) else {
            continue;
        };
        let style = PROPERTIES
            .iter()
            .filter_map(|&property| {
                computed
                    .get_property_value(property)
                    .ok()
                    .filter(|v| !v.is_empty())
                    .map(|v| format!("{property}:{v}"))
            })
            .collect::<Vec<_>>()
            .join(";");
        let _ = clone_el.set_attribute("style", &style);
    }

    let view_box = svg_el.get_attribute("viewBox")?;
    let mut parts = view_box
        .split_whitespace()
        .filter_map(|s| s.parse::<f64>().ok());
    let (_, _, width, height) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    // the live element only sets `width` (`network_map.rs`'s own `<svg>`), leaving its height to
    // the page's CSS/aspect-ratio handling — fine on the page itself, but a standalone file has
    // no such CSS around it to fall back on, so a viewer would otherwise guess a wrong one.
    clone.set_attribute("height", &height.to_string()).ok()?;

    let background = window
        .get_computed_style(&svg_el)
        .ok()
        .flatten()
        .and_then(|computed| computed.get_property_value("background-color").ok())
        .filter(|c| c != "rgba(0, 0, 0, 0)" && !c.is_empty())
        .unwrap_or_else(|| "white".into());
    let document = window.document()?;
    let background_rect = document
        .create_element_ns(Some("http://www.w3.org/2000/svg"), "rect")
        .ok()?;
    background_rect
        .set_attribute("width", &width.to_string())
        .ok()?;
    background_rect
        .set_attribute("height", &height.to_string())
        .ok()?;
    background_rect.set_attribute("fill", &background).ok()?;
    clone
        .insert_before(&background_rect, clone.first_child().as_ref())
        .ok()?;

    Some((
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}",
            clone.outer_html()
        ),
        width,
        height,
    ))
}

/// A click-and-remove on a throwaway `<a download>`: the standard way to make a browser save
/// bytes it already has (a blob or data URL) as a file, there being no plain "save this" call.
fn trigger_download(href: &str, filename: &str) -> Option<()> {
    let document = web_sys::window()?.document()?;
    let anchor: HtmlAnchorElement = document.create_element("a").ok()?.dyn_into().ok()?;
    anchor.set_href(href);
    anchor.set_download(filename);
    let body = document.body()?;
    body.append_child(&anchor).ok()?;
    anchor.click();
    body.remove_child(&anchor).ok()?;
    Some(())
}

/// Saves the map as a standalone SVG file: self-contained (see the module doc comment), opens in
/// a browser or any vector tool, stays crisp at any zoom.
pub fn download_svg(svg_ref: &NodeRef, filename: &str) {
    let Some((markup, ..)) = standalone_markup(svg_ref) else {
        return;
    };
    let parts = js_sys::Array::of1(&JsValue::from_str(&markup));
    let options = BlobPropertyBag::new();
    options.set_type("image/svg+xml;charset=utf-8");
    let Ok(blob) = Blob::new_with_str_sequence_and_options(&parts, &options) else {
        return;
    };
    let Ok(url) = Url::create_object_url_with_blob(&blob) else {
        return;
    };
    trigger_download(&url, filename);
    let _ = Url::revoke_object_url(&url);
}

/// Saves the map rasterized to a PNG file, at the same size its own coordinates already draw
/// it: simpler for a chat message or a document than a vector file, at the cost of staying
/// sharp only at that one size. Rasterizing is inherently asynchronous in a browser (there's no
/// synchronous "draw this SVG string onto a canvas") — an `<img>` has to load it first.
pub async fn download_png(svg_ref: &NodeRef, filename: &str) {
    let Some((markup, width, height)) = standalone_markup(svg_ref) else {
        return;
    };
    let parts = js_sys::Array::of1(&JsValue::from_str(&markup));
    let options = BlobPropertyBag::new();
    options.set_type("image/svg+xml;charset=utf-8");
    let Ok(blob) = Blob::new_with_str_sequence_and_options(&parts, &options) else {
        return;
    };
    let Ok(svg_url) = Url::create_object_url_with_blob(&blob) else {
        return;
    };

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Ok(img_el) = document.create_element("img") else {
        return;
    };
    let Ok(img) = img_el.dyn_into::<HtmlImageElement>() else {
        return;
    };
    img.set_src(&svg_url);
    // decoded, not just "loaded": only `decode()` guarantees the image is actually ready to
    // draw onto a canvas by the time its promise resolves.
    let decoded = JsFuture::from(img.decode()).await.is_ok();
    let _ = Url::revoke_object_url(&svg_url);
    if !decoded {
        return;
    }

    let Ok(canvas_el) = document.create_element("canvas") else {
        return;
    };
    let Ok(canvas) = canvas_el.dyn_into::<HtmlCanvasElement>() else {
        return;
    };
    canvas.set_width(width as u32);
    canvas.set_height(height as u32);
    let Ok(Some(context)) = canvas.get_context("2d") else {
        return;
    };
    let Ok(context) = context.dyn_into::<CanvasRenderingContext2d>() else {
        return;
    };
    if context
        .draw_image_with_html_image_element(&img, 0.0, 0.0)
        .is_err()
    {
        return;
    }
    let Ok(png_url) = canvas.to_data_url_with_type("image/png") else {
        return;
    };
    trigger_download(&png_url, filename);
}
