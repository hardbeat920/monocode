//! Settings and provider account operations over the app's native services.

mod accounts;
mod hosts;
pub(crate) mod updater;
pub use accounts::AccountsAdapter;
pub use hosts::SettingsAdapter;

pub fn check_for_updates(manual: bool, cx: &mut gpui::App) {
    updater::run(manual, std::rc::Rc::new(|_, _| {}), cx).detach();
}

pub(crate) fn convert<T: serde::Serialize, U: serde::de::DeserializeOwned>(value: T) -> U {
    serde_json::from_value(serde_json::to_value(value).expect("serializable native model"))
        .expect("matching view model")
}
