//! Perora's icon in the notification area, with its menu: Capture (with the shortcut), Open, and
//! Exit, as in the Tauri version. The left button shows the main window or puts it away; the right
//! one opens the menu.

use perora_core::language::Language;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::Signal;

/// The number the icon is embedded under (perora.rc).
const ICON_RESOURCE: u16 = 32512;

pub struct Tray {
    _icon: TrayIcon,
    capture: MenuItem,
    open: MenuItem,
    quit: MenuItem,
}

impl Tray {
    /// `registered`: the shortcut as shown, or `None` when another application has it.
    pub fn new(language: &Language, registered: Option<&str>, shortcut: &str) -> Option<Self> {
        let capture = MenuItem::new(language.capture(registered), true, None);
        let open = MenuItem::new(language.open(), true, None);
        let quit = MenuItem::new(language.quit(), true, None);
        let menu = Menu::new();
        menu.append_items(&[&capture, &open, &PredefinedMenuItem::separator(), &quit])
            .ok()?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip(tooltip(language, registered, shortcut))
            .with_icon(icon()?)
            .build()
            .ok()?;
        Some(Self {
            _icon: icon,
            capture,
            open,
            quit,
        })
    }

    /// Says it all again: in another language, or with the shortcut that captures now
    /// (`registered`: as shown, or `None` when another application has it).
    pub fn relabel(&self, language: &Language, registered: Option<&str>, shortcut: &str) {
        self.capture.set_text(language.capture(registered));
        self.open.set_text(language.open());
        self.quit.set_text(language.quit());
        let _ = self
            ._icon
            .set_tooltip(Some(tooltip(language, registered, shortcut)));
    }

    /// Sends what the menu and the icon are asked for. They call back outside GPUI's updates, so
    /// they only send; the application acts on it in its own time.
    pub fn listen(&self, sender: async_channel::Sender<Signal>) {
        let ids: [(MenuId, Signal); 3] = [
            (self.capture.id().clone(), Signal::Capture),
            (self.open.id().clone(), Signal::Open),
            (self.quit.id().clone(), Signal::Quit),
        ];
        let menu_sender = sender.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some((_, signal)) = ids.iter().find(|(id, _)| *id == event.id) {
                let _ = menu_sender.try_send(signal.clone());
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = sender.try_send(Signal::ToggleMain);
            }
        }));
    }
}

/// What the icon says: the name, and when the shortcut is not Perora's to use, that.
pub fn tooltip(language: &Language, registered: Option<&str>, shortcut: &str) -> String {
    match registered {
        Some(_) => "Perora".to_owned(),
        None => format!("Perora ({})", language.shortcut_unavailable(shortcut)),
    }
}

/// The application's icon at the size the notification area draws it: the icon has one drawn for
/// each small size, which is better than a big one made smaller.
fn icon() -> Option<Icon> {
    let side = perora_core::platform::small_icon_side();
    Icon::from_resource(ICON_RESOURCE, Some((side, side))).ok()
}
