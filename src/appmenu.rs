//! Leading-side application menu for ArcDockWin's desktop menu bar.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DOWN, VK_HOME, VK_RETURN, VK_UP};

use crate::panel::{Event, PanelContent, Widget};
use crate::topbar::{self, AppMenuAction};

const LAUNCHER: u32 = 1;
const CONTROL_CENTER: u32 = 2;
const TASK_MANAGER: u32 = 3;
const QUIT: u32 = 4;

pub struct AppMenu {
    selected: Option<usize>,
    bar: HWND,
}

impl AppMenu {
    pub fn new(bar: HWND) -> Self {
        Self {
            selected: None,
            bar,
        }
    }

    fn activate(&self, index: usize) {
        let action = match index {
            0 => AppMenuAction::Launcher,
            1 => AppMenuAction::ControlCenter,
            2 => AppMenuAction::TaskManager,
            3 => AppMenuAction::Quit,
            _ => return,
        };
        topbar::post_app_menu_action(action, self.bar);
    }
}

impl PanelContent for AppMenu {
    fn width(&self) -> f32 {
        252.0
    }

    fn build(&mut self) -> Vec<Widget> {
        vec![
            Widget::Heading {
                text: "ArcDockWin".into(),
                buttons: Vec::new(),
            },
            Widget::Menu {
                items: vec![
                    (LAUNCHER, Some(0xE71D), "Aplicativos".into()),
                    (
                        CONTROL_CENTER,
                        Some(crate::control::GLYPH),
                        "Central de Controle".into(),
                    ),
                    (TASK_MANAGER, Some(0xE9D9), "Gerenciador de tarefas".into()),
                ],
                selected: self.selected.filter(|i| *i < 3),
            },
            Widget::Separator,
            Widget::Menu {
                items: vec![(QUIT, Some(0xE7E8), "Sair do ArcDockWin".into())],
                selected: self.selected.filter(|i| *i == 3).map(|_| 0),
            },
        ]
    }

    fn event(&mut self, event: Event) -> bool {
        match event {
            Event::Click(id) => {
                let index = match id {
                    LAUNCHER => 0,
                    CONTROL_CENTER => 1,
                    TASK_MANAGER => 2,
                    QUIT => 3,
                    _ => return false,
                };
                self.activate(index);
                false
            }
            Event::Key(key) if key == VK_UP.0 || key == VK_DOWN.0 => {
                let len = 4;
                self.selected = Some(if key == VK_UP.0 {
                    self.selected
                        .map_or(len - 1, |selected| (selected + len - 1) % len)
                } else {
                    self.selected.map_or(0, |selected| (selected + 1) % len)
                });
                true
            }
            Event::Key(key) if key == VK_HOME.0 => {
                self.selected = Some(0);
                true
            }
            Event::Key(key) if key == VK_RETURN.0 => {
                self.activate(self.selected.unwrap_or(0));
                false
            }
            Event::PointerMove if self.selected.is_some() => {
                self.selected = None;
                true
            }
            _ => false,
        }
    }
}
