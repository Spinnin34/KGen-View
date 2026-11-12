use windows::Win32::Foundation::POINT;

use crate::commands;

use super::{icons::Icon, layout::ChromeLayout};

#[derive(Clone, Copy)]
pub struct ToolbarItem {
    pub command: usize,
    pub group: u8,
    pub icon: Icon,
    pub label: &'static str,
}

const ITEMS: [ToolbarItem; 11] = [
    ToolbarItem {
        command: commands::OPEN,
        group: 0,
        icon: Icon::Open,
        label: "Abrir",
    },
    ToolbarItem {
        command: commands::SAVE_AS,
        group: 0,
        icon: Icon::Save,
        label: "Guardar como",
    },
    ToolbarItem {
        command: commands::PREVIOUS,
        group: 1,
        icon: Icon::Previous,
        label: "Anterior",
    },
    ToolbarItem {
        command: commands::NEXT,
        group: 1,
        icon: Icon::Next,
        label: "Siguiente",
    },
    ToolbarItem {
        command: commands::INFO,
        group: 2,
        icon: Icon::Info,
        label: "Información",
    },
    ToolbarItem {
        command: commands::COPY,
        group: 2,
        icon: Icon::Copy,
        label: "Copiar imagen",
    },
    ToolbarItem {
        command: commands::COPY_URL,
        group: 2,
        icon: Icon::CopyUrl,
        label: "Copiar enlace directo",
    },
    ToolbarItem {
        command: commands::ROTATE_LEFT,
        group: 3,
        icon: Icon::RotateLeft,
        label: "Girar a la izquierda",
    },
    ToolbarItem {
        command: commands::ROTATE_RIGHT,
        group: 3,
        icon: Icon::RotateRight,
        label: "Girar a la derecha",
    },
    ToolbarItem {
        command: commands::FULLSCREEN,
        group: 3,
        icon: Icon::Fullscreen,
        label: "Pantalla completa",
    },
    ToolbarItem {
        command: commands::UPDATE,
        group: 4,
        icon: Icon::Download,
        label: "Descargar actualización",
    },
];

pub fn toolbar_items() -> &'static [ToolbarItem] {
    &ITEMS
}

#[derive(Default)]
pub struct ToolbarState {
    pub hovered: Option<usize>,
    pub pressed: Option<usize>,
}

impl ToolbarState {
    pub fn hit_test(&self, layout: &ChromeLayout, point: POINT) -> Option<usize> {
        layout.toolbar_items.iter().find_map(|placed| {
            placed
                .rect
                .contains(point)
                .then_some(ITEMS[placed.index].command)
        })
    }

    pub fn update_hover(&mut self, layout: &ChromeLayout, point: POINT) -> bool {
        let next = self.hit_test(layout, point);
        let changed = next != self.hovered;
        self.hovered = next;
        changed
    }

    pub fn press(&mut self, layout: &ChromeLayout, point: POINT) -> Option<usize> {
        self.pressed = self.hit_test(layout, point);
        self.pressed
    }

    pub fn release(&mut self, layout: &ChromeLayout, point: POINT) -> Option<usize> {
        let released = self.hit_test(layout, point);
        let activated = (released == self.pressed).then_some(released).flatten();
        self.pressed = None;
        activated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_ids_are_unique() {
        let mut ids: Vec<_> = ITEMS.iter().map(|item| item.command).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ITEMS.len());
    }
}
