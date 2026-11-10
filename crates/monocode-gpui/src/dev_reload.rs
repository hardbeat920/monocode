//! The development runner owns stdin; a command or EOF requests normal shutdown.
//! Release builds do not include this hook.
use gpui::App;

pub fn install(cx: &mut App) {
    if std::env::var("MONOCODE_DEV_RELOAD").as_deref() != Ok("1") {
        return;
    }
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let mut command = String::new();
        let _ = std::io::stdin().read_line(&mut command);
        let _ = sender.send_blocking(());
    });
    cx.spawn(async move |cx| {
        if receiver.recv().await.is_ok() {
            cx.update(crate::quit);
        }
    })
    .detach();
}
