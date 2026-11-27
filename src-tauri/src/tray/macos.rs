use objc2::rc::Retained;
use objc2::{define_class, msg_send, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSImage, NSImageScaling, NSImageView, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSData, NSPoint, NSSize};
use tauri::tray::TrayIcon;

define_class!(
    // SAFETY: This view has no additional state or subclassing requirements.
    #[unsafe(super(NSImageView))]
    #[thread_kind = MainThreadOnly]
    struct UnreadAccent;

    impl UnreadAccent {
        // Keep all input on the native status button beneath the decoration.
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> bool {
            false
        }
    }
);

pub(super) fn template_bytes(unread: bool) -> &'static [u8] {
    if unread {
        include_bytes!("../../icons/tray.png")
    } else {
        include_bytes!("../../icons/tray-idle.png")
    }
}

pub(super) fn set_unread_appearance(tray: &TrayIcon, unread: bool) {
    let _ = tray.with_inner_tray_icon(move |inner| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(button) = inner.ns_status_item().and_then(|item| item.button(mtm)) else {
            return;
        };
        let existing = button
            .subviews()
            .iter()
            .find_map(|view| view.downcast::<UnreadAccent>().ok());
        let Some(current_template) = button.image() else {
            return;
        };
        let size = current_template.size();
        let was_unread = existing.as_ref().is_some_and(|accent| !accent.isHidden());
        if unread != was_unread {
            let Some(template) = load_image(template_bytes(unread), size) else {
                return;
            };
            template.setTemplate(true);
            button.setImage(Some(&template));
        }

        if !unread {
            if let Some(accent) = existing {
                accent.setHidden(true);
            }
            return;
        }

        button.layoutSubtreeIfNeeded();
        let Some(cell) = button.cell() else {
            return;
        };
        let frame = cell.imageRectForBounds(button.bounds());
        if let Some(accent) = existing {
            accent.setFrame(frame);
            accent.setHidden(false);
            return;
        }

        let Some(image) = load_image(include_bytes!("../../icons/tray-accent.png"), size) else {
            return;
        };
        // SAFETY: Calling NSImageView's initializer on its main-thread subclass.
        let accent: Retained<UnreadAccent> =
            unsafe { msg_send![UnreadAccent::alloc(mtm), initWithFrame: frame] };
        accent.setImage(Some(&image));
        accent.setImageScaling(NSImageScaling::ScaleNone);
        accent.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewMaxXMargin
                | NSAutoresizingMaskOptions::ViewMinYMargin
                | NSAutoresizingMaskOptions::ViewMaxYMargin,
        );
        button.addSubview_positioned_relativeTo(&accent, NSWindowOrderingMode::Above, None);
    });
}

fn load_image(bytes: &[u8], size: NSSize) -> Option<Retained<NSImage>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
    image.setSize(size);
    Some(image)
}
