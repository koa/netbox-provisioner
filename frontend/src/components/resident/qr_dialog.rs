//! Scope: The QR code a phone's camera joins a WLAN with, in a dialog with its name and
//! password; not the WLAN's fields (`networks`).
use i18n::resident as t;
use patternfly_yew::prelude::{
    Alert, AlertType, Bullseye, Button, ButtonVariant, Modal, ModalVariant,
};
use qrcode::{Color, EcLevel, QrCode};
use yew::{Callback, Html, Properties, function_component, html};

#[derive(Debug, Clone, PartialEq, Properties)]
pub struct QrDialogProps {
    pub ssid: String,
    /// none: an open WLAN
    pub psk: Option<String>,
    /// with a draft of the WLAN: the code holds the draft, which the devices don't have yet
    pub unsaved: bool,
    pub on_close: Callback<()>,
}

/// The WLAN's QR code, its name and password.
#[function_component(QrDialog)]
pub fn qr_dialog(props: &QrDialogProps) -> Html {
    let code = qr_path(&wifi_text(&props.ssid, props.psk.as_deref())).map(|path| {
        let border = 4;
        let view_box = format!(
            "-{border} -{border} {size} {size}",
            size = path.width + 2 * border
        );
        html! {
            <svg
                width="240"
                height="240"
                viewBox={view_box}
                role="img"
                aria-label={t::qr_label(&props.ssid)}
                shape-rendering="crispEdges"
                class="resident-qr"
            >
                <path d={path.d} fill="#000000"/>
            </svg>
        }
    });
    let unsaved = props.unsaved.then(|| {
        html! {
            <Alert inline=true plain=true r#type={AlertType::Warning} title={t::QR_UNSAVED}/>
        }
    });
    let psk = props.psk.as_ref().map(|psk| {
        html! {
            <div class="pf-v6-c-description-list__group">
                <dt class="pf-v6-c-description-list__term">
                    <span class="pf-v6-c-description-list__text">{t::PSK}</span>
                </dt>
                <dd class="pf-v6-c-description-list__description">
                    <div class="pf-v6-c-description-list__text"><code>{psk}</code></div>
                </dd>
            </div>
        }
    });
    let footer = html! {
        <Button
            variant={ButtonVariant::Primary}
            label={t::DONE}
            onclick={props.on_close.reform(|_| ())}
        />
    };
    html! {
        <div class="pf-v6-c-backdrop resident-qr-dialog">
            <Bullseye>
                <Modal
                    title={t::qr_title(&props.ssid)}
                    description={t::QR_HINT}
                    variant={ModalVariant::Small}
                    onclose={props.on_close.clone()}
                    {footer}
                >
                    <div class="resident-qr-body">
                        {unsaved}
                        {code}
                        <dl class="pf-v6-c-description-list pf-m-horizontal pf-m-compact">
                            <div class="pf-v6-c-description-list__group">
                                <dt class="pf-v6-c-description-list__term">
                                    <span class="pf-v6-c-description-list__text">{t::WLAN}</span>
                                </dt>
                                <dd class="pf-v6-c-description-list__description">
                                    <div class="pf-v6-c-description-list__text">{&props.ssid}</div>
                                </dd>
                            </div>
                            {psk}
                        </dl>
                    </div>
                </Modal>
            </Bullseye>
        </div>
    }
}

/// What a phone's camera reads as a WLAN to join: `WIFI:T:WPA;S:…;P:…;;`, an open one with
/// `T:nopass`; `\`, `;`, `,`, `:` and `"` escaped.
fn wifi_text(ssid: &str, psk: Option<&str>) -> String {
    let escape = |text: &str| {
        text.chars().fold(String::new(), |mut out, c| {
            if matches!(c, '\\' | ';' | ',' | ':' | '"') {
                out.push('\\');
            }
            out.push(c);
            out
        })
    };
    match psk {
        Some(psk) => format!("WIFI:T:WPA;S:{};P:{};;", escape(ssid), escape(psk)),
        None => format!("WIFI:T:nopass;S:{};;", escape(ssid)),
    }
}

/// A QR code's dark modules as an SVG path, one unit each.
#[derive(Debug, PartialEq)]
struct QrPath {
    d: String,
    width: usize,
}

/// The QR code of `text` with error correction M; none where it doesn't fit one.
fn qr_path(text: &str) -> Option<QrPath> {
    let code = QrCode::with_error_correction_level(text, EcLevel::M).ok()?;
    let width = code.width();
    let d = code
        .to_colors()
        .iter()
        .enumerate()
        .filter(|(_, color)| **color == Color::Dark)
        .map(|(i, _)| format!("M{} {}h1v1h-1z", i % width, i / width))
        .collect();
    Some(QrPath { d, width })
}

#[cfg(test)]
mod test {
    use super::*;

    /// The characters the format uses itself are escaped, in name and password.
    #[test]
    fn wifi_text_escapes() {
        assert_eq!(
            wifi_text(r#"Meier;Gast"#, Some(r#"a:b,c"d\e"#)),
            r#"WIFI:T:WPA;S:Meier\;Gast;P:a\:b\,c\"d\\e;;"#
        );
        assert_eq!(wifi_text("Offen", None), "WIFI:T:nopass;S:Offen;;");
    }

    /// The smallest code holding a WLAN is 25 modules wide or more, with its three finder
    /// patterns dark at the corners.
    #[test]
    fn a_path_per_dark_module() {
        let path = qr_path(&wifi_text("lab-wlan", Some("0123456789abcdef"))).unwrap();
        assert!(path.width >= 25, "{}", path.width);
        assert!(path.d.starts_with("M0 0h1v1h-1z"), "{}", &path.d[..40]);
        let right = format!("M{} 0h1v1h-1z", path.width - 1);
        assert!(path.d.contains(&right));
    }
}
