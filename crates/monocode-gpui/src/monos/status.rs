//! AppKit owns menu navigation and accessibility; actions return to GPUI.
use objc2::runtime::AnyObject;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, sel,
};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::{NSData, NSSize, NSString};
use std::rc::Rc;
struct Action {
    key: String,
    run: Rc<dyn Fn(&str)>,
}
define_class!(
    #[unsafe(super(NSMenuItem))]
    #[name="MonocodeMonoMenuItem"]
    #[thread_kind=MainThreadOnly]
    #[ivars=Action]
    struct Item;
    impl Item{
        #[unsafe(method(runMonoAction:))]
        fn run(&self,_:Option<&AnyObject>){(self.ivars().run)(&self.ivars().key);}
    }
);
impl Item {
    fn new(
        mtm: MainThreadMarker,
        title: &str,
        key: String,
        run: Rc<dyn Fn(&str)>,
    ) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(Action { key, run });
        let item: Retained<Self> = unsafe {
            msg_send![super(this),initWithTitle:&*NSString::from_str(title),action:Some(sel!(runMonoAction:)),keyEquivalent:&*NSString::from_str("")]
        };
        // The status menu retains each item for as long as it can be invoked.
        unsafe {
            item.setTarget(Some(&item));
        }
        item
    }
}
pub(super) struct Status {
    item: Retained<NSStatusItem>,
    signature: String,
}
impl Status {
    pub(super) fn new() -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.);
        if let Some(button) = item.button(mtm) {
            let data = NSData::with_bytes(include_bytes!(
                "../../../../src-tauri/macos/AppIcon.icon/Assets/icon.png"
            ));
            if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                image.setSize(NSSize::new(18., 18.));
                image.setTemplate(true);
                button.setImage(Some(&image));
            } else {
                button.setTitle(&NSString::from_str("Mono"));
            }
            button.setToolTip(Some(&NSString::from_str("MonoCode")));
        }
        Some(Self {
            item,
            signature: String::new(),
        })
    }
    pub(super) fn update(&mut self, monos: &[(String, String)], run: Rc<dyn Fn(&str)>) {
        let signature = format!("{monos:?}");
        if self.signature == signature {
            return;
        }
        self.signature = signature;
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        if monos.is_empty() {
            let title = NSMenuItem::new(mtm);
            title.setTitle(&NSString::from_str("No monos yet"));
            title.setEnabled(false);
            menu.addItem(&title);
        }
        for (id, name) in monos {
            let item = Item::new(mtm, name, id.clone(), run.clone());
            menu.addItem(&item);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&Item::new(
            mtm,
            "Quick Composer",
            "composer".into(),
            run.clone(),
        ));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&Item::new(mtm, "Quit MonoCode", "quit".into(), run));
        self.item.setMenu(Some(&menu));
    }
}
impl Drop for Status {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
