use crate::model;

stella2_meta::designer_impl! {
    crate::view::toolbar::ToolbarView
}

impl ToolbarView {
    /// Handle `init` event.
    fn init(&self) {}

    /// Handle `toggle_sidebar_button.activate` event.
    fn toggle_sidebar(&self) {
        // Toggle the sidebar
        let visible = self.wnd_state().sidebar_visible;
        self.raise_dispatch(model::WndAction::ToggleSidebar(!visible));
    }
}
