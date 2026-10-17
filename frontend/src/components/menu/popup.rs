use crate::pages::routes::AppRoute;
use patternfly_yew::prelude::{Divider, Icon, MenuToggle, MenuToggleVariant};
use std::fmt::Display;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement, Node};
use yew::events::{FocusEvent, KeyboardEvent, MouseEvent};
use yew::{
    AttrValue, Callback, Component, Context, ContextHandle, ContextProvider, Html, NodeRef,
    Properties, classes, function_component, html, use_context,
};
use yew_nested_router::prelude::RouterContext;

#[derive(Properties, PartialEq)]
pub struct PopupMenuProps {
    #[prop_or_default]
    pub text: Option<Html>,
    #[prop_or_default]
    pub icon: Option<Html>,
    #[prop_or_default]
    pub aria_label: AttrValue,
    #[prop_or_default]
    pub variant: MenuToggleVariant,
    #[prop_or_default]
    pub disabled: bool,
    /// `MenuGroup`s of `MenuLinkItem`s
    #[prop_or_default]
    pub children: Html,
}

/// Closes the surrounding `PopupMenu`, provided to its items.
#[derive(Clone, PartialEq)]
struct CloseMenu(Callback<()>);

/// A PatternFly `MenuToggle` with a `Menu` below it, used instead of patternfly-yew's `Dropdown`.
///
/// That one positions the menu with popper.js in a portal and recreates its popper modifier
/// closure on every render, which logged errors to the console when the menu was opened or a
/// click in it rebuilt the surrounding page (the breadcrumb). A dropdown menu
/// opens downwards, so plain CSS positioning is enough (`.popup-menu` in `local.css`). Taken
/// from cable-editor.
pub struct PopupMenu {
    is_open: bool,
    /// Set when the menu was opened, so `rendered` focuses an entry once it exists.
    focus_on_render: bool,
    root_ref: NodeRef,
    toggle_ref: NodeRef,
    menu_ref: NodeRef,
}

pub enum PopupMenuMsg {
    Toggle,
    Close,
    FocusOut(FocusEvent),
    KeyDown(KeyboardEvent),
}

impl Component for PopupMenu {
    type Message = PopupMenuMsg;
    type Properties = PopupMenuProps;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            is_open: false,
            focus_on_render: false,
            root_ref: NodeRef::default(),
            toggle_ref: NodeRef::default(),
            menu_ref: NodeRef::default(),
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            PopupMenuMsg::Toggle => {
                self.is_open = !self.is_open;
                self.focus_on_render = self.is_open;
                true
            }
            PopupMenuMsg::Close => self.close(),
            PopupMenuMsg::FocusOut(e) => {
                let focus_inside = e
                    .related_target()
                    .and_then(|target| target.dyn_into::<Node>().ok())
                    .zip(self.root_ref.cast::<Node>())
                    .is_some_and(|(target, root)| root.contains(Some(&target)));
                !focus_inside && self.close()
            }
            PopupMenuMsg::KeyDown(e) => self.on_key_down(&e),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let link = ctx.link();
        let props = ctx.props();
        let class = classes!("popup-menu");
        let close = CloseMenu(link.callback(|()| PopupMenuMsg::Close));
        html! {
            <div
                ref={self.root_ref.clone()}
                {class}
                onfocusout={link.callback(PopupMenuMsg::FocusOut)}
                onkeydown={link.callback(PopupMenuMsg::KeyDown)}
            >
                <MenuToggle
                    r#ref={self.toggle_ref.clone()}
                    variant={props.variant}
                    text={props.text.clone()}
                    icon={props.icon.clone()}
                    aria_label={props.aria_label.clone()}
                    disabled={props.disabled}
                    expanded={self.is_open}
                    ontoggle={link.callback(|()| PopupMenuMsg::Toggle)}
                />
                if self.is_open {
                    <div ref={self.menu_ref.clone()} class="pf-v6-c-menu pf-m-scrollable" tabindex="-1">
                        <div class="pf-v6-c-menu__content">
                            <ContextProvider<CloseMenu> context={close}>
                                {props.children.clone()}
                            </ContextProvider<CloseMenu>>
                        </div>
                    </div>
                }
            </div>
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        // patternfly-yew's MenuToggle has no `title` prop; the tooltip names the toggle like its
        // `aria-label` does, e.g. when a phone hides the text or it is only a symbol
        if let Some(toggle) = self.toggle_ref.cast::<Element>()
            && !ctx.props().aria_label.is_empty()
        {
            let _ = toggle.set_attribute("title", &ctx.props().aria_label);
        }
        // Focus the selected entry, so the arrow keys work right away and leaving the menu with
        // the focus closes it. Without one the menu itself, as a focused first entry would look
        // selected; the arrow keys then start there.
        if std::mem::take(&mut self.focus_on_render)
            && let Some(menu) = self.menu_ref.cast::<Element>()
        {
            let target = menu_items(&menu, ".pf-v6-c-menu__item.pf-m-selected")
                .pop()
                .unwrap_or(menu);
            if let Ok(target) = target.dyn_into::<HtmlElement>() {
                let _ = target.focus();
            }
        }
    }
}

impl PopupMenu {
    /// Returns whether the view has to be rerendered.
    fn close(&mut self) -> bool {
        std::mem::replace(&mut self.is_open, false)
    }

    /// Escape closes the menu and returns the focus to the toggle, the arrow keys, Home and End
    /// move the focus between its entries. Returns whether the view has to be rerendered.
    fn on_key_down(&mut self, e: &KeyboardEvent) -> bool {
        let Some(menu) = self.menu_ref.cast::<Element>().filter(|_| self.is_open) else {
            return false;
        };
        let items = menu_items(&menu, "[role=menuitem]");
        let current = gloo::utils::document()
            .active_element()
            .and_then(|focused| items.iter().position(|item| *item == focused));
        let next = match e.key().as_str() {
            "Escape" => {
                e.prevent_default();
                if let Some(toggle) = self.toggle_ref.cast::<HtmlElement>() {
                    let _ = toggle.focus();
                }
                return self.close();
            }
            "ArrowDown" => current.map_or(0, |i| (i + 1) % items.len()),
            "ArrowUp" => current.map_or(items.len().wrapping_sub(1), |i| {
                (i + items.len() - 1) % items.len()
            }),
            "Home" => 0,
            "End" => items.len().wrapping_sub(1),
            _ => return false,
        };
        e.prevent_default();
        if let Some(item) = items
            .get(next)
            .and_then(|item| item.dyn_ref::<HtmlElement>())
        {
            let _ = item.focus();
        }
        false
    }
}

fn menu_items(menu: &Element, selector: &str) -> Vec<Element> {
    let Ok(items) = menu.query_selector_all(selector) else {
        return Vec::new();
    };
    (0..items.length())
        .filter_map(|i| items.item(i))
        .filter_map(|item| item.dyn_into::<Element>().ok())
        .collect()
}

#[derive(Properties, PartialEq)]
pub struct MenuGroupProps {
    #[prop_or_default]
    pub title: Option<AttrValue>,
    /// Separate the group from the one before by a divider
    #[prop_or_default]
    pub divider: bool,
    /// `MenuLinkItem`s
    #[prop_or_default]
    pub children: Html,
}

/// A list of entries in a `PopupMenu`, optionally with a title.
#[function_component]
pub fn MenuGroup(props: &MenuGroupProps) -> Html {
    html! {
        <>
            if props.divider {
                <Divider/>
            }
            <section class="pf-v6-c-menu__group">
                if let Some(title) = &props.title {
                    <h1 class="pf-v6-c-menu__group-title">{title.clone()}</h1>
                }
                <ul class="pf-v6-c-menu__list" role="menu">{props.children.clone()}</ul>
            </section>
        </>
    }
}

fn item_main(selected: bool, children: &Html) -> Html {
    html! {
        <span class="pf-v6-c-menu__item-main">
            <span class="pf-v6-c-menu__item-text">{children.clone()}</span>
            if selected {
                <span class="pf-v6-c-menu__item-select-icon">{Icon::Check}</span>
            }
        </span>
    }
}

fn item_class(selected: bool) -> yew::Classes {
    let mut class = classes!("pf-v6-c-menu__item");
    if selected {
        class.push("pf-m-selected");
    }
    class
}

#[derive(Properties, PartialEq)]
pub struct SelectProps<T: Clone + PartialEq + Display + 'static> {
    #[prop_or_default]
    pub placeholder: Option<AttrValue>,
    pub entries: Vec<T>,
    #[prop_or_default]
    pub selected: Option<T>,
    pub onselect: Callback<T>,
}

/// A select on a `PopupMenu`, used instead of patternfly-yew's `SimpleSelect`: that one's menu is
/// a popper with `position: fixed`, which the page doesn't grow for and which moves along when
/// it scrolls, so a list longer than the window below the toggle (all devices, all ports) had
/// entries out of reach. This menu scrolls in itself (`pf-m-scrollable`) and lies in the page.
#[function_component]
pub fn Select<T: Clone + PartialEq + Display + 'static>(props: &SelectProps<T>) -> Html {
    let text = props
        .selected
        .as_ref()
        .map(|selected| AttrValue::from(selected.to_string()))
        .or_else(|| props.placeholder.clone())
        .unwrap_or_default();
    let entries = props
        .entries
        .iter()
        .map(|entry| {
            let onclick = props.onselect.reform({
                let entry = entry.clone();
                move |()| entry.clone()
            });
            html! {
                <MenuActionItem selected={props.selected.as_ref() == Some(entry)} {onclick}>
                    {entry.to_string()}
                </MenuActionItem>
            }
        })
        .collect::<Html>();
    html! {
        <PopupMenu text={html!(text)}>
            <MenuGroup>{entries}</MenuGroup>
        </PopupMenu>
    }
}

#[derive(Properties, PartialEq)]
pub struct MenuActionItemProps {
    #[prop_or_default]
    pub selected: bool,
    pub onclick: Callback<()>,
    #[prop_or_default]
    pub children: Html,
}

/// Entry of a `PopupMenu` calling `onclick` and closing the menu.
#[function_component]
pub fn MenuActionItem(props: &MenuActionItemProps) -> Html {
    let close = use_context::<CloseMenu>();
    let onclick = {
        let onclick = props.onclick.clone();
        Callback::from(move |_: MouseEvent| {
            if let Some(CloseMenu(close)) = &close {
                close.emit(());
            }
            onclick.emit(());
        })
    };
    html! {
        <li class="pf-v6-c-menu__list-item" role="none">
            <button class={item_class(props.selected)} type="button" role="menuitem" tabindex="-1" {onclick}>
                {item_main(props.selected, &props.children)}
            </button>
        </li>
    }
}

#[derive(Properties, PartialEq)]
pub struct MenuLinkItemProps {
    pub to: AppRoute,
    /// Mark as selected although `to` is not the current page (e.g. the site of the device shown)
    #[prop_or_default]
    pub selected: bool,
    #[prop_or_default]
    pub children: Html,
}

/// Entry of a `PopupMenu` navigating to `to`; marked as selected when that is the current page.
pub struct MenuLinkItem {
    router: Option<RouterContext<AppRoute>>,
    _router_handle: Option<ContextHandle<RouterContext<AppRoute>>>,
    close: Option<CloseMenu>,
}

pub enum MenuLinkItemMsg {
    /// The route changed, which may change whether this entry is selected
    RouterChanged(RouterContext<AppRoute>),
    Click(MouseEvent),
}

impl Component for MenuLinkItem {
    type Message = MenuLinkItemMsg;
    type Properties = MenuLinkItemProps;

    fn create(ctx: &Context<Self>) -> Self {
        let (router, router_handle) = ctx
            .link()
            .context::<RouterContext<AppRoute>>(ctx.link().callback(MenuLinkItemMsg::RouterChanged))
            .unzip();
        let close = ctx
            .link()
            .context::<CloseMenu>(Callback::noop())
            .map(|(close, _)| close);
        Self {
            router,
            _router_handle: router_handle,
            close,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            MenuLinkItemMsg::RouterChanged(router) => {
                self.router = Some(router);
                true
            }
            MenuLinkItemMsg::Click(e) => {
                // Modified clicks keep the browser behaviour (e.g. open in a new tab)
                if e.ctrl_key() || e.meta_key() || e.shift_key() || e.button() != 0 {
                    return false;
                }
                e.prevent_default();
                if let Some(CloseMenu(close)) = &self.close {
                    close.emit(());
                }
                if let Some(router) = &self.router {
                    router.push(ctx.props().to.clone());
                }
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let props = ctx.props();
        let selected = props.selected || self.router.as_ref().is_some_and(|r| r.is_same(&props.to));
        let href = self
            .router
            .as_ref()
            .map(|r| r.render_target(props.to.clone()));
        let onclick = ctx.link().callback(MenuLinkItemMsg::Click);
        html! {
            <li class="pf-v6-c-menu__list-item" role="none">
                <a class={item_class(selected)} {href} role="menuitem" tabindex="-1" {onclick}>
                    {item_main(selected, &props.children)}
                </a>
            </li>
        }
    }
}
