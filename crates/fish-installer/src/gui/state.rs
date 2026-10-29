//! State and text of the installer window (no drawing here).

use std::path::PathBuf;

use crate::flow::{Kind, Options, State};

/// UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// Russian.
    Ru,
}

impl Lang {
    /// Picks the language from a locale name such as `ru-RU` or `en_US.UTF-8`.
    pub fn from_locale(locale: Option<&str>) -> Lang {
        match locale.map(|l| l.to_ascii_lowercase()) {
            Some(l) if l.starts_with("ru") => Lang::Ru,
            _ => Lang::En,
        }
    }
}

/// Texts of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    /// Window title.
    Title,
    /// Launcher path label.
    Launcher,
    /// Browse button.
    Browse,
    /// Instance name label.
    Instance,
    /// Checkbox: select Fish in the launcher.
    Select,
    /// Edition label.
    Edition,
    /// Dropdown entry: the newest edition.
    Newest,
    /// Button: load the list of editions.
    Refresh,
    /// Install button.
    Install,
    /// Repair button.
    Repair,
    /// Uninstall button.
    Uninstall,
    /// Button shown after success.
    OpenLauncher,
    /// Shown while working.
    Working,
    /// Shown after success.
    Done,
    /// Shown after failure.
    Failed,
    /// Error: no launcher chosen.
    NeedLauncher,
    /// Error: the launcher path is not a file.
    LauncherMissing,
    /// Error: empty instance name.
    NeedInstance,
}

/// The text `t` in `lang`.
pub fn tr(lang: Lang, t: Text) -> &'static str {
    use Lang::{En, Ru};
    use Text::*;
    match (t, lang) {
        (Title, En) => "Fish installer",
        (Title, Ru) => "Установщик Fish",
        (Launcher, En) => "Horizon launcher",
        (Launcher, Ru) => "Лаунчер Horizon",
        (Browse, En) => "Browse…",
        (Browse, Ru) => "Обзор…",
        (Instance, En) => "Instance name",
        (Instance, Ru) => "Имя экземпляра",
        (Select, En) => "Select Fish in the launcher",
        (Select, Ru) => "Выбрать Fish в лаунчере",
        (Edition, En) => "Edition",
        (Edition, Ru) => "Издание",
        (Newest, En) => "Newest (recommended)",
        (Newest, Ru) => "Новейшее (рекомендуется)",
        (Refresh, En) => "Load list",
        (Refresh, Ru) => "Загрузить список",
        (Install, En) => "Install",
        (Install, Ru) => "Установить",
        (Repair, En) => "Repair",
        (Repair, Ru) => "Исправить",
        (Uninstall, En) => "Uninstall",
        (Uninstall, Ru) => "Удалить",
        (OpenLauncher, En) => "Open Horizon launcher",
        (OpenLauncher, Ru) => "Открыть лаунчер Horizon",
        (Working, En) => "Working…",
        (Working, Ru) => "Выполняется…",
        (Done, En) => "Done.",
        (Done, Ru) => "Готово.",
        (Failed, En) => "Failed:",
        (Failed, Ru) => "Ошибка:",
        (NeedLauncher, En) => "Choose the Horizon launcher first.",
        (NeedLauncher, Ru) => "Сначала выберите лаунчер Horizon.",
        (LauncherMissing, En) => "That file does not exist.",
        (LauncherMissing, Ru) => "Такого файла нет.",
        (NeedInstance, En) => "Give the instance a name.",
        (NeedInstance, Ru) => "Укажите имя экземпляра.",
    }
}

/// Which edition the player picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// The newest one for this platform.
    Newest,
    /// A specific id from the release index.
    Id(String),
    /// A local zip.
    Zip(PathBuf),
}

/// What the window is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the player.
    Idle,
    /// A flow is running in the background.
    Running,
    /// A flow ended.
    Finished {
        /// Whether it succeeded.
        ok: bool,
    },
}

/// Everything the window shows and edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiState {
    /// UI language.
    pub lang: Lang,
    /// Launcher path as typed.
    pub launcher: String,
    /// Instance name.
    pub instance: String,
    /// Make Play start the instance.
    pub select: bool,
    /// Chosen edition.
    pub choice: Choice,
    /// Edition ids loaded from the index.
    pub known_editions: Vec<String>,
    /// Progress log.
    pub log: Vec<String>,
    /// Current phase.
    pub phase: Phase,
    /// The recorded installation, if any.
    pub installed: Option<State>,
}

impl GuiState {
    /// A fresh window state.
    pub fn new(lang: Lang, launcher: Option<PathBuf>, installed: Option<State>) -> GuiState {
        GuiState {
            lang,
            launcher: launcher
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            instance: installed
                .as_ref()
                .map_or_else(|| "Fish".to_string(), |s| s.instance.clone()),
            select: true,
            choice: Choice::Newest,
            known_editions: Vec::new(),
            log: Vec::new(),
            phase: Phase::Idle,
            installed,
        }
    }

    /// The text `t` in this window's language.
    pub fn t(&self, t: Text) -> &'static str {
        tr(self.lang, t)
    }

    /// What the main button does: repair when Fish is installed, else install.
    pub fn primary_action(&self) -> Kind {
        if self.installed.is_some() {
            Kind::Repair
        } else {
            Kind::Install
        }
    }

    /// Checks the inputs; returns the launcher path.
    pub fn validate(&self) -> Result<PathBuf, &'static str> {
        let launcher = self.launcher.trim().trim_matches('"');
        if launcher.is_empty() {
            return Err(self.t(Text::NeedLauncher));
        }
        let path = PathBuf::from(launcher);
        if !path.is_file() {
            return Err(self.t(Text::LauncherMissing));
        }
        if self.instance.trim().is_empty() {
            return Err(self.t(Text::NeedInstance));
        }
        Ok(path)
    }

    /// Builds the installer options for the current choices.
    pub fn options(&self, home: PathBuf, launcher: &std::path::Path) -> Options {
        Options {
            home,
            instance: self.instance.trim().to_string(),
            select: self.select,
            force: false,
            allow_unsigned: std::env::var("FISH_LOADER_ALLOW_UNSIGNED").is_ok_and(|v| v == "1"),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            edition_zip: match &self.choice {
                Choice::Zip(p) => Some(p.clone()),
                _ => None,
            },
            edition_id: match &self.choice {
                Choice::Id(id) => Some(id.clone()),
                _ => None,
            },
            releases_url: std::env::var("FISH_LOADER_RELEASES_URL").ok(),
            launcher_exe: launcher.display().to_string(),
        }
    }

    /// Begins a run: validates and switches to [`Phase::Running`].
    pub fn start(&mut self) -> Result<PathBuf, &'static str> {
        let launcher = self.validate()?;
        self.log.clear();
        self.phase = Phase::Running;
        Ok(launcher)
    }

    /// Appends a progress line.
    pub fn push(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
    }

    /// Ends a run with its result.
    pub fn finish(&mut self, result: Result<State, String>) {
        match result {
            Ok(state) => {
                self.push(self.t(Text::Done));
                self.installed = Some(state);
                self.phase = Phase::Finished { ok: true };
            }
            Err(e) => {
                self.push(format!("{} {e}", self.t(Text::Failed)));
                self.phase = Phase::Finished { ok: false };
            }
        }
    }

    /// Ends an uninstall.
    pub fn finish_uninstall(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.push(self.t(Text::Done));
                self.installed = None;
                self.phase = Phase::Finished { ok: true };
            }
            Err(e) => self.finish(Err(e)),
        }
    }

    /// Records the editions the index offers.
    pub fn set_known_editions(&mut self, ids: Vec<String>) {
        if let Choice::Id(id) = &self.choice
            && !ids.contains(id)
        {
            self.choice = Choice::Newest;
        }
        self.known_editions = ids;
    }

    /// True while a flow runs (buttons are disabled).
    pub fn busy(&self) -> bool {
        self.phase == Phase::Running
    }

    /// True after a successful run: the launcher can be opened.
    pub fn can_open_launcher(&self) -> bool {
        self.phase == Phase::Finished { ok: true } && self.installed.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn installed() -> State {
        State {
            schema: 1,
            launcher: "l".into(),
            instance: "Mine".into(),
            edition: "0.1.0+horizon.x".into(),
            horizon_tag: "t".into(),
            installed_at: 0,
        }
    }

    #[test]
    fn the_language_follows_the_locale() {
        assert_eq!(Lang::from_locale(Some("ru-RU")), Lang::Ru);
        assert_eq!(Lang::from_locale(Some("RU_ru.UTF-8")), Lang::Ru);
        assert_eq!(Lang::from_locale(Some("en-US")), Lang::En);
        assert_eq!(Lang::from_locale(Some("de")), Lang::En);
        assert_eq!(Lang::from_locale(None), Lang::En);
    }

    #[test]
    fn every_text_exists_in_both_languages_and_differs() {
        use Text::*;
        for t in [
            Title,
            Launcher,
            Browse,
            Instance,
            Select,
            Edition,
            Newest,
            Refresh,
            Install,
            Repair,
            Uninstall,
            OpenLauncher,
            Working,
            Done,
            Failed,
            NeedLauncher,
            LauncherMissing,
            NeedInstance,
        ] {
            let (en, ru) = (tr(Lang::En, t), tr(Lang::Ru, t));
            assert!(!en.is_empty() && !ru.is_empty());
            assert_ne!(en, ru, "{t:?} is not translated");
        }
    }

    #[test]
    fn defaults_and_the_primary_action() {
        let fresh = GuiState::new(Lang::En, None, None);
        assert_eq!(
            (fresh.instance.as_str(), fresh.select),
            ("Fish", true),
            "Select is on by default"
        );
        assert_eq!(fresh.primary_action(), Kind::Install);
        assert_eq!(fresh.choice, Choice::Newest);
        let again = GuiState::new(Lang::Ru, Some("C:/l.exe".into()), Some(installed()));
        assert_eq!(again.primary_action(), Kind::Repair);
        assert_eq!(again.instance, "Mine");
        assert_eq!(again.launcher, "C:/l.exe");
    }

    #[test]
    fn validation_reports_the_first_problem_in_the_players_language() {
        let mut s = GuiState::new(Lang::Ru, None, None);
        assert_eq!(s.validate(), Err("Сначала выберите лаунчер Horizon."));
        s.launcher = "/definitely/not/here".into();
        assert_eq!(s.validate(), Err("Такого файла нет."));
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("horizon-launcher");
        std::fs::write(&exe, b"x").unwrap();
        s.launcher = format!("\"{}\"", exe.display());
        s.instance = "  ".into();
        assert_eq!(s.validate(), Err("Укажите имя экземпляра."));
        s.instance = "Fish".into();
        assert_eq!(
            s.validate().unwrap(),
            exe,
            "quotes around a pasted path are ignored"
        );
        s.lang = Lang::En;
        s.instance.clear();
        assert_eq!(s.validate(), Err("Give the instance a name."));
    }

    #[test]
    fn a_run_moves_through_the_phases() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("l");
        std::fs::write(&exe, b"x").unwrap();
        let mut s = GuiState::new(Lang::En, Some(exe.clone()), None);
        s.push("old");
        assert_eq!(s.start().unwrap(), exe);
        assert!(s.busy() && s.log.is_empty(), "starting clears the log");
        s.push("Unpacking");
        s.finish(Err("boom".into()));
        assert_eq!(s.phase, Phase::Finished { ok: false });
        assert_eq!(s.log.last().unwrap(), "Failed: boom");
        assert!(!s.can_open_launcher());
        s.start().unwrap();
        s.finish(Ok(installed()));
        assert!(s.can_open_launcher() && s.installed.is_some());
        assert_eq!(s.log.last().unwrap(), "Done.");
        s.finish_uninstall(Ok(()));
        assert!(s.installed.is_none() && !s.can_open_launcher());
        s.finish_uninstall(Err("nope".into()));
        assert_eq!(s.phase, Phase::Finished { ok: false });
        s.launcher.clear();
        assert!(s.start().is_err());
        assert_eq!(
            s.phase,
            Phase::Finished { ok: false },
            "a failed validation does not start a run"
        );
    }

    #[test]
    fn edition_choices_become_installer_options() {
        let mut s = GuiState::new(Lang::En, None, None);
        let o = s.options("/home".into(), Path::new("/l"));
        assert_eq!((o.edition_zip.clone(), o.edition_id.clone()), (None, None));
        assert_eq!(o.instance, "Fish");
        s.choice = Choice::Id("0.1.0+horizon.abc".into());
        assert_eq!(
            s.options("/home".into(), Path::new("/l"))
                .edition_id
                .as_deref(),
            Some("0.1.0+horizon.abc")
        );
        s.choice = Choice::Zip("/e.zip".into());
        assert_eq!(
            s.options("/home".into(), Path::new("/l")).edition_zip,
            Some(PathBuf::from("/e.zip"))
        );
        // the list refresh drops a choice the index no longer has
        s.choice = Choice::Id("gone".into());
        s.set_known_editions(vec!["a".into(), "b".into()]);
        assert_eq!(
            (s.choice.clone(), s.known_editions.len()),
            (Choice::Newest, 2)
        );
        s.choice = Choice::Id("a".into());
        s.set_known_editions(vec!["a".into()]);
        assert_eq!(s.choice, Choice::Id("a".into()));
    }
}
