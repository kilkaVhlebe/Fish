//! Drawing and threading of the installer window (excluded from coverage: pixels).

use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

use super::state::{Choice, GuiState, Lang, Text};
use crate::flow::{self, Installer, Kind, Reporter};
use crate::launcher::{LauncherCli, Probes, ProcessLauncherCli};
use crate::source::{self, HttpFetcher};

enum Msg {
    Line(String),
    Installed(Result<flow::State, String>),
    Uninstalled(Result<(), String>),
    Editions(Result<Vec<String>, String>),
}

struct ChannelReporter(Sender<Msg>);

impl Reporter for ChannelReporter {
    fn step(&self, message: &str) {
        let _ = self.0.send(Msg::Line(message.to_string()));
    }
}

struct App {
    state: GuiState,
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
}

impl App {
    fn new() -> App {
        let (tx, rx) = channel();
        let lang = Lang::from_locale(sys_locale::get_locale().as_deref());
        let home = fish_launch::fish_home();
        let launcher = crate::launcher::discover(None, &Probes::here(), &|p| {
            crate::launcher::is_working_launcher(p)
        })
        .ok();
        App {
            state: GuiState::new(lang, launcher, flow::read_state(&home)),
            rx,
            tx,
        }
    }

    fn spawn_flow(&mut self, kind: Option<Kind>) {
        let Ok(launcher_exe) = self.state.start() else {
            if let Err(e) = self.state.validate() {
                self.state.push(e);
            }
            self.state.phase = super::state::Phase::Idle;
            return;
        };
        let home = fish_launch::fish_home();
        let options = self.state.options(home, &launcher_exe);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let launcher = ProcessLauncherCli::new(&launcher_exe);
            let reporter = ChannelReporter(tx.clone());
            let installer = Installer {
                launcher: &launcher,
                fetcher: &HttpFetcher,
                reporter: &reporter,
                options,
                launch: match crate::cli::launch_binary() {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = tx.send(Msg::Installed(Err(e.to_string())));
                        return;
                    }
                },
                self_exe: std::env::current_exe().ok(),
                key: source::RELEASE_KEY,
                github: source::RELEASE_GITHUB,
            };
            match kind {
                Some(k) => {
                    let _ = tx.send(Msg::Installed(installer.run(k).map_err(|e| e.to_string())));
                }
                None => {
                    let _ = tx.send(Msg::Uninstalled(
                        installer.uninstall().map_err(|e| e.to_string()),
                    ));
                }
            }
        });
    }

    fn load_editions(&self) {
        let tx = self.tx.clone();
        let url = std::env::var("FISH_LOADER_RELEASES_URL").ok();
        let unsigned = std::env::var("FISH_LOADER_ALLOW_UNSIGNED").is_ok_and(|v| v == "1");
        std::thread::spawn(move || {
            let result = source::releases_base(url.as_deref(), source::RELEASE_GITHUB)
                .and_then(|base| {
                    source::load_index(&HttpFetcher, &base, source::RELEASE_KEY, unsigned)
                })
                .map(|index| {
                    index
                        .editions
                        .into_iter()
                        .filter(|e| !e.server)
                        .map(|e| e.id)
                        .collect()
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Msg::Editions(result));
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Line(l) => self.state.push(l),
                Msg::Installed(r) => self.state.finish(r),
                Msg::Uninstalled(r) => self.state.finish_uninstall(r),
                Msg::Editions(Ok(ids)) => self.state.set_known_editions(ids),
                Msg::Editions(Err(e)) => self.state.push(e),
            }
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let busy = self.state.busy();
        ui.heading(self.state.t(Text::Title));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(self.state.t(Text::Launcher));
            ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.state.launcher).desired_width(360.0),
            );
            if ui
                .add_enabled(!busy, egui::Button::new(self.state.t(Text::Browse)))
                .clicked()
                && let Some(p) = rfd::FileDialog::new().pick_file()
            {
                self.state.launcher = p.display().to_string();
            }
        });
        ui.horizontal(|ui| {
            ui.label(self.state.t(Text::Instance));
            ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.state.instance).desired_width(160.0),
            );
        });
        let select_label = self.state.t(Text::Select);
        ui.add_enabled(
            !busy,
            egui::Checkbox::new(&mut self.state.select, select_label),
        );
        ui.horizontal(|ui| {
            ui.label(self.state.t(Text::Edition));
            let selected = match &self.state.choice {
                Choice::Newest => self.state.t(Text::Newest).to_string(),
                Choice::Id(id) => id.clone(),
                Choice::Zip(p) => p.display().to_string(),
            };
            ui.add_enabled_ui(!busy, |ui| {
                egui::ComboBox::from_id_salt("edition")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        let newest = self.state.t(Text::Newest);
                        ui.selectable_value(&mut self.state.choice, Choice::Newest, newest);
                        for id in self.state.known_editions.clone() {
                            ui.selectable_value(&mut self.state.choice, Choice::Id(id.clone()), id);
                        }
                    });
            });
            if ui
                .add_enabled(!busy, egui::Button::new(self.state.t(Text::Refresh)))
                .clicked()
            {
                self.load_editions();
            }
            if ui.add_enabled(!busy, egui::Button::new("zip…")).clicked()
                && let Some(p) = rfd::FileDialog::new()
                    .add_filter("zip", &["zip"])
                    .pick_file()
            {
                self.state.choice = Choice::Zip(p);
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = match self.state.primary_action() {
                Kind::Repair => Text::Repair,
                _ => Text::Install,
            };
            if ui
                .add_enabled(!busy, egui::Button::new(self.state.t(label)))
                .clicked()
            {
                let kind = self.state.primary_action();
                self.spawn_flow(Some(kind));
            }
            if self.state.installed.is_some()
                && ui
                    .add_enabled(!busy, egui::Button::new(self.state.t(Text::Uninstall)))
                    .clicked()
            {
                self.spawn_flow(None);
            }
            if self.state.can_open_launcher()
                && ui.button(self.state.t(Text::OpenLauncher)).clicked()
            {
                let _ = std::process::Command::new(self.state.launcher.trim().trim_matches('"'))
                    .spawn();
            }
            if busy {
                ui.spinner();
                ui.label(self.state.t(Text::Working));
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.state.log {
                    ui.label(line);
                }
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        if self.state.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.draw(ui);
    }
}

/// Opens the window; returns the process exit code.
pub fn run_window() -> i32 {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    let title = super::state::tr(
        Lang::from_locale(sys_locale::get_locale().as_deref()),
        Text::Title,
    );
    match eframe::run_native(title, options, Box::new(|_cc| Ok(Box::new(App::new())))) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: cannot open the window: {e}");
            1
        }
    }
}

/// Keeps the trait import used on platforms where it would otherwise be unused.
#[allow(dead_code)]
fn _assert_launcher_cli(_: &dyn LauncherCli) {}
