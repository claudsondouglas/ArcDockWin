//! Bandeja: painel da seta à esquerda dos ícones da barra. Mostra os ícones ocultos (clique
//! esquerdo e direito vão para o app, como na barra do Windows) e, na engrenagem, a lista de
//! todos com um interruptor "fica na barra". A escolha vai para o SQLite (colordb).

use crate::panel::{Event, PanelContent, Widget, glyph};
use crate::topbar::{self, TrayButton};

const EDIT: u32 = 1;
const BACK: u32 = 2;
/// Ícone oculto `i` → id `ICON + i`.
const ICON: u32 = 100;
/// Interruptor do ícone `i` → id `SHOW + i`.
const SHOW: u32 = 10_000;

pub struct Overflow {
    edit: bool,
}

impl Overflow {
    pub fn new() -> Self {
        Self { edit: false }
    }
}

/// Nome do ícone: 1ª linha da dica ou, sem dica, o exe.
fn name(e: &topbar::TrayEntry) -> String {
    let tip = e.tip.lines().next().unwrap_or("").trim();
    if !tip.is_empty() {
        return tip.to_string();
    }
    let exe = e.key.split('#').next().unwrap_or("");
    if exe.is_empty() { "Ícone".into() } else { exe.trim_end_matches(".exe").into() }
}

impl PanelContent for Overflow {
    fn width(&self) -> f32 {
        if self.edit { 300.0 } else { 196.0 }
    }

    fn build(&mut self) -> Vec<Widget> {
        let list = topbar::tray_list();
        if self.edit {
            let mut out = vec![Widget::Heading { text: "Ícones na barra".into(), buttons: vec![(glyph::CHEVRON_LEFT, BACK)] }];
            out.extend(list.into_iter().enumerate().map(|(i, e)| Widget::Toggle {
                id: SHOW + i as u32,
                text: name(&e),
                on: e.shown,
                icon: Some(e.icon),
            }));
            return out;
        }
        let hidden: Vec<_> =
            list.into_iter().enumerate().filter(|(_, e)| !e.visible).map(|(i, e)| (ICON + i as u32, e.icon)).collect();
        let mut out = vec![Widget::Heading { text: "Bandeja".into(), buttons: vec![(glyph::SETTINGS, EDIT)] }];
        if hidden.is_empty() {
            out.push(Widget::Text("Nenhum ícone oculto. Escolha na engrenagem quais saem da barra.".into()));
        } else {
            out.push(Widget::Icons { items: hidden });
        }
        out
    }

    fn event(&mut self, ev: Event) -> bool {
        match ev {
            Event::Click(EDIT) => {
                self.edit = true;
                true
            }
            Event::Click(BACK) => {
                self.edit = false;
                true
            }
            // O app que abre janela ou menu toma o primeiro plano e o painel fecha sozinho.
            Event::Click(id) if (ICON..SHOW).contains(&id) => {
                topbar::tray_click_index((id - ICON) as usize, TrayButton::Left);
                false
            }
            Event::RightClick(id) if (ICON..SHOW).contains(&id) => {
                topbar::tray_click_index((id - ICON) as usize, TrayButton::Right);
                false
            }
            Event::Toggle(id, on) if id >= SHOW => {
                topbar::set_tray_shown((id - SHOW) as usize, on);
                true
            }
            _ => false,
        }
    }
}
