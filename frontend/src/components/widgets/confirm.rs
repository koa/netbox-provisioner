use crate::i18n::common::CANCEL;
use patternfly_yew::prelude::{Button, ButtonVariant};
use web_sys::MouseEvent;
use yew::{Callback, Html, html};

/// A button that asks first: `ask` until clicked, then `question` with `now` and one to cancel.
pub struct Confirm {
    pub asking: bool,
    pub question: String,
    pub now: &'static str,
    pub ask: String,
    pub disabled: bool,
}

impl Confirm {
    /// `on_now` does it, `on_asking` gets whether it asks now (`true`) or was cancelled.
    pub fn view(self, on_now: Callback<MouseEvent>, on_asking: Callback<bool>) -> Html {
        if self.asking {
            html! {<>
                {self.question}
                <Button
                    variant={ButtonVariant::Danger}
                    label={self.now}
                    disabled={self.disabled}
                    onclick={on_now}
                />
                {" "}
                <Button
                    variant={ButtonVariant::Link}
                    label={CANCEL}
                    onclick={on_asking.reform(|_| false)}
                />
            </>}
        } else {
            html! {
                <Button
                    variant={ButtonVariant::Secondary}
                    label={self.ask}
                    disabled={self.disabled}
                    onclick={on_asking.reform(|_| true)}
                />
            }
        }
    }
}
