use anyhow::{Context as _, Result};
use tao::{
    event_loop::EventLoop,
    platform::windows::{WindowBuilderExtWindows, WindowExtWindows},
    window::{Window, WindowBuilder},
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        SetWindowLongW, GWL_EXSTYLE, GWL_STYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    },
};
use wry::WebViewBuilder;

use crate::UserEvent;

pub fn create_candidate_window(event_loop: &EventLoop<UserEvent>) -> Result<Window> {
    let window = WindowBuilder::new()
        .with_decorations(false)
        .with_title("azooKey Candidates")
        .with_focused(false)
        .with_visible(false)
        .with_undecorated_shadow(false)
        .with_transparent(true)
        .build(event_loop)
        .context("failed to create candidate window")?;

    let hwnd = HWND(window.hwnd() as *mut std::ffi::c_void);
    unsafe {
        let ex_style = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0;
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style as i32);
        SetWindowLongW(hwnd, GWL_STYLE, WS_POPUP.0 as i32);
    }

    Ok(window)
}

pub fn create_candidate_webview<'a>() -> Result<WebViewBuilder<'a>> {
    Ok(WebViewBuilder::new().with_transparent(true).with_html(
        r##"
<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
html, body { margin: 0; padding: 0; background: transparent; font-family: "Segoe UI", "Yu Gothic UI", sans-serif; }
body { padding: 7px; }
main {
  box-sizing: border-box;
  min-width: 230px;
  max-height: 260px;
  overflow: hidden;
  background: rgba(255,255,255,.98);
  border: 1px solid rgba(0,0,0,.14);
  border-radius: 9px;
  box-shadow: 0 4px 16px rgba(0,0,0,.16);
}
ol { margin: 0; padding: 5px; list-style: none; max-height: 248px; overflow-y: auto; }
li {
  display: grid;
  grid-template-columns: 1.6rem minmax(0,1fr) auto;
  gap: .45rem;
  align-items: baseline;
  padding: 5px 7px;
  border-radius: 5px;
  font-size: 14px;
}
li[data-selected] { background: #dceeff; outline: 1px solid #67aef8; }
.number { color: #777; font-size: 11px; }
.text { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.annotation { color: #777; font-size: 11px; padding-left: .6rem; white-space: nowrap; }
@media (prefers-color-scheme: dark) {
  main { background: rgba(31,31,31,.98); border-color: rgba(255,255,255,.2); color: #eee; }
  li[data-selected] { background: #263f61; outline-color: #5c96d9; }
  .number, .annotation { color: #aaa; }
}
</style>
<script>
function updateCandidates(items) {
  const list = document.getElementById("list");
  list.replaceChildren();
  items.forEach((item, index) => {
    const li = document.createElement("li");
    li.dataset.index = index;

    const number = document.createElement("span");
    number.className = "number";
    number.textContent = index < 9 ? String(index + 1) : "";

    const text = document.createElement("span");
    text.className = "text";
    text.textContent = item.text;

    const annotation = document.createElement("span");
    annotation.className = "annotation";
    annotation.textContent = item.annotation || "";

    li.append(number, text, annotation);
    list.appendChild(li);
  });
}
function updateSelection(index) {
  const list = document.getElementById("list");
  list.querySelectorAll("[data-selected]").forEach(e => e.removeAttribute("data-selected"));
  const item = list.children[index];
  if (item) {
    item.setAttribute("data-selected", "");
    item.scrollIntoView({block: "nearest"});
  }
}
</script>
</head>
<body><main><ol id="list"></ol></main></body>
</html>
"##,
    ))
}
