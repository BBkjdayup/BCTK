use tauri::Manager;
use webview2_com::{
    ContextMenuRequestedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR,
        ICoreWebView2_11, ICoreWebView2ContextMenuItem, ICoreWebView2ContextMenuItemCollection,
    },
    take_pwstr,
};
use windows_core::{Interface, PWSTR};

/// 精简 Windows WebView2 的原生右键菜单，同时保留“更多工具”子菜单，
/// 以继续使用 WebView2 提供的语音输入等系统能力。
pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("未找到主窗口，已跳过 WebView2 右键菜单精简。");
        return Ok(());
    };

    window.with_webview(|platform_webview| {
        if let Err(error) = unsafe { install_for_webview(&platform_webview) } {
            eprintln!("WebView2 右键菜单精简失败：{error}");
        }
    })
}

unsafe fn install_for_webview(
    platform_webview: &tauri::webview::PlatformWebview,
) -> windows_core::Result<()> {
    let core_webview = unsafe { platform_webview.controller().CoreWebView2()? };
    let core_webview_11: ICoreWebView2_11 = core_webview.cast()?;
    let handler = ContextMenuRequestedEventHandler::create(Box::new(|_, event_args| {
        if let Some(event_args) = event_args {
            let menu_items = unsafe { event_args.MenuItems()? };
            unsafe { filter_menu_items(&menu_items)? };
        }
        Ok(())
    }));

    let mut token = 0;
    unsafe { core_webview_11.add_ContextMenuRequested(&handler, &mut token)? };
    Ok(())
}

unsafe fn filter_menu_items(
    menu_items: &ICoreWebView2ContextMenuItemCollection,
) -> windows_core::Result<()> {
    let mut count = collection_count(menu_items)?;

    // 倒序删除，避免删除项目后后续索引发生偏移。
    for index in (0..count).rev() {
        let item = unsafe { menu_items.GetValueAtIndex(index)? };
        if is_separator(&item)? {
            continue;
        }

        let name = item_text(&item, |item, value| unsafe { item.Name(value) });
        let label = item_text(&item, |item, value| unsafe { item.Label(value) });
        let should_keep = match (name, label) {
            (Some(name), Some(label)) => should_keep_command(&name, &label),
            // Reading menu metadata can fail for third-party/system items. Keep an
            // unreadable item instead of silently deleting a command we could not identify.
            _ => true,
        };
        if !should_keep {
            unsafe { menu_items.RemoveValueAtIndex(index)? };
        }
    }

    // 删除过滤后遗留在开头、结尾或连续出现的分隔线。
    count = collection_count(menu_items)?;
    let mut index = 0;
    let mut previous_was_separator = true;
    while index < count {
        let item = unsafe { menu_items.GetValueAtIndex(index)? };
        let separator = is_separator(&item)?;
        if separator && previous_was_separator {
            unsafe { menu_items.RemoveValueAtIndex(index)? };
            count -= 1;
            continue;
        }

        previous_was_separator = separator;
        index += 1;
    }

    if count > 0 {
        let last_item = unsafe { menu_items.GetValueAtIndex(count - 1)? };
        if is_separator(&last_item)? {
            unsafe { menu_items.RemoveValueAtIndex(count - 1)? };
        }
    }

    Ok(())
}

fn collection_count(
    menu_items: &ICoreWebView2ContextMenuItemCollection,
) -> windows_core::Result<u32> {
    let mut count = 0;
    unsafe { menu_items.Count(&mut count)? };
    Ok(count)
}

fn is_separator(item: &ICoreWebView2ContextMenuItem) -> windows_core::Result<bool> {
    let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND::default();
    unsafe { item.Kind(&mut kind)? };
    Ok(kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR)
}

fn item_text(
    item: &ICoreWebView2ContextMenuItem,
    read: impl FnOnce(&ICoreWebView2ContextMenuItem, *mut PWSTR) -> windows_core::Result<()>,
) -> Option<String> {
    let mut value = PWSTR::null();
    if read(item, &mut value).is_err() {
        return None;
    }
    Some(take_pwstr(value))
}

fn should_keep_command(name: &str, label: &str) -> bool {
    let normalized_name = name.trim().to_ascii_lowercase();
    if matches!(
        normalized_name.as_str(),
        "undo"
            | "cut"
            | "copy"
            | "paste"
            | "pasteasplaintext"
            | "pastematchstyle"
            | "pasteandmatchstyle"
            | "selectall"
            | "moretools"
    ) {
        return true;
    }

    let normalized_label = label
        .replace(['&', '…'], "")
        .replace("...", "")
        .trim()
        .to_lowercase();

    matches!(
        normalized_label.as_str(),
        "撤销"
            | "undo"
            | "剪切"
            | "cut"
            | "复制"
            | "copy"
            | "粘贴"
            | "paste"
            | "粘贴为纯文本"
            | "paste as plain text"
            | "paste and match style"
            | "全选"
            | "select all"
            | "更多工具"
            | "more tools"
    )
}

#[cfg(test)]
mod tests {
    use super::should_keep_command;

    #[test]
    fn keeps_the_approved_native_commands() {
        for name in [
            "undo",
            "cut",
            "copy",
            "paste",
            "pasteAsPlainText",
            "pasteAndMatchStyle",
            "selectAll",
            "moreTools",
        ] {
            assert!(should_keep_command(name, ""), "应保留 {name}");
        }
    }

    #[test]
    fn keeps_localized_fallback_labels() {
        for label in [
            "撤销",
            "剪切",
            "复制",
            "粘贴",
            "粘贴为纯文本",
            "全选",
            "更多工具",
        ] {
            assert!(should_keep_command("", label), "应保留 {label}");
        }
    }

    #[test]
    fn removes_unapproved_native_commands() {
        for (name, label) in [
            ("emoji", "表情符号"),
            ("writingDirection", "书写方向"),
            ("sendTabToSelf", "发送标签页到你的设备"),
            ("inspectElement", "检查"),
        ] {
            assert!(!should_keep_command(name, label), "应删除 {label}");
        }
    }
}
