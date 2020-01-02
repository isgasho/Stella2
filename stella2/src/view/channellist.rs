use std::ops::Range;
use tcw3::{
    ui::{
        mixins::scrollwheel::ScrollAxisFlags,
        prelude::*,
        theming,
        views::{table, table::LineTy, Button, Label},
    },
    uicore::{HView, SizeTraits},
};

use crate::stylesheet::elem_id;

stella2_meta::designer_impl! {
    crate::view::channellist::ChannelListView
}

impl ChannelListView {
    fn init(&self) {
        // Set up the table model
        {
            let mut edit = self.table().table().edit().unwrap();
            edit.set_model(TableModelQuery {
                style_manager: self.style_manager(),
            });
            edit.insert(LineTy::Row, 0..29);
            edit.insert(LineTy::Col, 0..1);
            edit.set_scroll_pos([0.0, edit.scroll_pos()[1]]);
        }
    }
}

struct TableModelQuery {
    style_manager: &'static theming::Manager,
}

impl table::TableModelQuery for TableModelQuery {
    fn new_view(&mut self, cell: table::CellIdx) -> (HView, Box<dyn table::CellCtrler>) {
        let label = Label::new(self.style_manager);
        label.set_text(format!("Item {}", cell[1]));

        let wrap = theming::StyledBox::new(self.style_manager, Default::default());
        wrap.set_child(theming::Role::Generic, Some(&label));
        wrap.set_class_set(
            if cell[1] % 4 == 0 {
                elem_id::SIDEBAR_GROUP_HEADER
            } else {
                elem_id::SIDEBAR_ITEM
            } | if cell[1] == 1 || (cell[1] % 4 == 0 && cell[1] < 28) {
                theming::ClassSet::ACTIVE
            } else {
                theming::ClassSet::empty()
            },
        );

        let button = if cell[1] % 4 == 0 {
            let button = Button::new(self.style_manager);
            // Clear `.BUTTON` and replace with `#SIDEBAR_GROUP_BULLET`
            button.set_class_set(elem_id::SIDEBAR_GROUP_BULLET);

            wrap.set_child(theming::Role::Bullet, Some(&button));

            Some(button)
        } else {
            None
        };

        (wrap.view().clone(), Box::new(((wrap, button),)))
    }

    fn range_size(&mut self, line_ty: LineTy, range: Range<u64>, _approx: bool) -> f64 {
        match line_ty {
            LineTy::Row => (range.start..range.end)
                .map(|i| if i % 4 == 0 { 25.0 } else { 20.0 })
                .sum(),

            // TODO: find a better way to fill the width
            LineTy::Col => (range.end - range.start) as f64 * 50.0,
        }
    }
}