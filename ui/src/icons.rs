//! The icons the design draws inline (Lucide paths, 24px viewbox, stroke only).

use leptos::prelude::*;

pub const WALLET: &str = "M19 7V4a1 1 0 0 0-1-1H5a2 2 0 0 0 0 4h15a1 1 0 0 1 1 1v4h-3a2 2 0 0 0 0 4h3a1 1 0 0 0 1-1v-2a1 1 0 0 0-1-1M3 5v14a2 2 0 0 0 2 2h15a1 1 0 0 0 1-1v-4";
pub const UP_DOWN: &str = "M7 15l5 5 5-5M7 9l5-5 5 5";
pub const PANEL_RIGHT: &str = "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2zM15 3v18";
pub const BELL: &str = "M10.268 21a2 2 0 0 0 3.464 0M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326";
pub const BACK: &str = "M19 12H5M12 19l-7-7 7-7";
pub const PLUS: &str = "M5 12h14M12 5v14";
pub const X: &str = "M18 6 6 18M6 6l12 12";
pub const REPEAT: &str = "m17 2 4 4-4 4M3 11v-1a4 4 0 0 1 4-4h14M7 22l-4-4 4-4M21 13v1a4 4 0 0 1-4 4H3";
pub const PREV: &str = "M15 18l-6-6 6-6";
pub const NEXT: &str = "M9 18l6-6-6-6";
pub const TRASH: &str = "M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2";
pub const TRASH_LINES: &str = "M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M10 11v6M14 11v6";
pub const CHECK: &str = "M20 6 9 17l-5-5";
pub const MOON: &str = "M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401";
pub const SUN: &str = "M12 8a4 4 0 1 0 0 8a4 4 0 0 0 0-8zM12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41";
pub const DOWNLOAD: &str = "M12 15V3M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5";
pub const SIGN_OUT: &str = "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9";
pub const COPY: &str = "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2zM4 16a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2";
pub const CLIP: &str = "M13.234 20.252 21 12.3M16 6l-8.414 8.586a2 2 0 0 0 0 2.828a2 2 0 0 0 2.828 0l8.414-8.586a4 4 0 0 0 0-5.656a4 4 0 0 0-5.656 0l-8.415 8.585a6 6 0 1 0 8.486 8.486";
pub const USER: &str = "M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2M12 3a4 4 0 1 0 0 8a4 4 0 0 0 0-8z";
pub const SLIDERS: &str = "M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4";
pub const LOCK: &str = "M7 11V7a5 5 0 0 1 10 0v4M5 11h14a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2z";
pub const USERS: &str = "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M9 3a4 4 0 1 0 0 8a4 4 0 0 0 0-8zM22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75";
pub const PLUG: &str = "M12 22v-5M9 8V2M15 8V2M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8z";
pub const PACKAGE: &str = "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16zM3.3 7l8.7 5 8.7-5M12 22V12";
pub const BOLT: &str = "M13 2 3 14h9l-1 8 10-12h-9l1-8z";

/// A decorative stroke icon. The meaning belongs in adjacent text or the control's `aria-label`.
#[component]
pub fn Ico(d: &'static str, #[prop(optional)] small: bool, #[prop(optional)] large: bool) -> impl IntoView {
    let class = if small { "d-icon d-icon--sm" } else if large { "d-icon d-icon--lg" } else { "d-icon" };
    view! { <span class=class aria-hidden="true"><svg viewBox="0 0 24 24" focusable="false"><path d=d></path></svg></span> }
}

/// The pebblelab/fin mark: three dots rising.
#[component]
pub fn Mark() -> impl IntoView {
    view! {
        <svg viewBox="0 0 24 24" width="20" height="20" focusable="false" aria-hidden="true" style="flex:none">
            <circle cx="4.5" cy="19" r="1.5" fill="var(--text-subtle)"></circle>
            <circle cx="10" cy="14.5" r="2.25" fill="var(--text-secondary)"></circle>
            <circle cx="17.5" cy="7.5" r="4" fill="var(--accent-primary)"></circle>
        </svg>
    }
}

/// A person's avatar content: their picture, or the grey placeholder figure.
#[component]
pub fn Face(#[prop(into)] picture: Signal<Option<String>>) -> impl IntoView {
    move || match picture.get() {
        Some(src) => view! { <img src=src alt=""/> }.into_any(),
        None => view! {
            <svg viewBox="0 0 24 24" focusable="false" aria-hidden="true" style="width:100%;height:100%">
                <circle cx="12" cy="9" r="4" fill="var(--neutral-8)"></circle>
                <path d="M4 23c0-4.4 3.6-8 8-8s8 3.6 8 8z" fill="var(--neutral-8)"></path>
            </svg>
        }.into_any(),
    }
}
