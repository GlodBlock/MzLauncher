use eframe::egui;
use serde_json::{json, Value};

use crate::app::{MzApp, NewInstanceTab};
use crate::model::ModProject;
use crate::pages::home::loader_key;

const SIDEBAR_WIDTH: f32 = 180.0;
const SOURCE_BUTTON_HEIGHT: f32 = 44.0;
const FILTER_COMBO_WIDTH: f32 = 150.0;
const CUSTOM_FIELD_WIDTH: f32 = 180.0;
const ACTION_BUTTON_SIZE: egui::Vec2 = egui::Vec2::splat(36.0);
const MODPACK_ICON_SIZE: egui::Vec2 = egui::Vec2::new(56.0, 56.0);
const MODPACK_ROW_HEIGHT: f32 = 76.0;
const NEW_INSTANCE_WINDOW_SIZE: [f32; 2] = [1040.0, 700.0];
const NEW_INSTANCE_MIN_WINDOW_SIZE: [f32; 2] = [760.0, 520.0];

impl MzApp {
    pub fn new_instance_window(&mut self, ctx: &egui::Context) {
        if self.create.is_none() {
            return;
        }
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("new_instance_window"),
            egui::ViewportBuilder::default()
                .with_title(self.tr("create.title"))
                .with_inner_size(NEW_INSTANCE_WINDOW_SIZE)
                .with_min_inner_size(NEW_INSTANCE_MIN_WINDOW_SIZE),
            |ctx, _class| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    self.new_instance_page(ui);
                });
                if ctx.input(|input| input.viewport().close_requested()) {
                    self.create = None;
                }
            },
        );
    }

    fn new_instance_page(&mut self, ui: &mut egui::Ui) {
        let tab = self
            .create
            .as_ref()
            .map(|create| create.tab)
            .unwrap_or(NewInstanceTab::CurseForge);
        let mut selected_tab = None;
        egui::SidePanel::left("new_instance_sources")
            .resizable(false)
            .exact_width(SIDEBAR_WIDTH)
            .show_inside(ui, |ui| {
                for source in [
                    NewInstanceTab::CurseForge,
                    NewInstanceTab::Ftb,
                    NewInstanceTab::Modrinth,
                    NewInstanceTab::Custom,
                ] {
                    let label = match source {
                        NewInstanceTab::CurseForge => "CurseForge".into(),
                        NewInstanceTab::Ftb => "FTB".into(),
                        NewInstanceTab::Modrinth => "Modrinth".into(),
                        NewInstanceTab::Custom => self.tr("create.custom"),
                    };
                    if ui
                        .add_sized(
                            [ui.available_width(), SOURCE_BUTTON_HEIGHT],
                            egui::SelectableLabel::new(tab == source, label),
                        )
                        .clicked()
                    {
                        selected_tab = Some(source);
                    }
                    ui.add_space(crate::layout::CARD_LIST_GAP);
                }
            });
        egui::CentralPanel::default().show_inside(ui, |ui| match tab {
            NewInstanceTab::Custom => self.custom_instance_form(ui),
            _ => self.modpack_browser(ui),
        });
        if let Some(tab) = selected_tab {
            self.select_new_instance_tab(tab);
        }
    }

    fn select_new_instance_tab(&mut self, tab: NewInstanceTab) {
        if self.create.as_ref().is_some_and(|create| create.tab == tab) {
            return;
        }
        if let Some(create) = &mut self.create {
            create.tab = tab;
        }
        if tab.provider().is_some() {
            self.search_modpacks();
        }
    }

    pub fn search_modpacks(&mut self) {
        let Some(provider) = self
            .create
            .as_ref()
            .and_then(|create| create.tab.provider())
        else {
            return;
        };
        if self.modpack_browser.provider != provider {
            self.modpack_browser.provider = provider.into();
            self.modpack_browser.query.clear();
            self.modpack_browser.game_version.clear();
            self.modpack_browser.loader.clear();
        }
        self.modpack_browser.generation += 1;
        let generation = self.modpack_browser.generation;
        self.modpack_browser.results.clear();
        self.modpack_browser.searching = true;
        self.call(
            "modpack.search",
            json!({
                "provider": provider,
                "query": self.modpack_browser.query.trim(),
                "gameVersion": optional_filter(&self.modpack_browser.game_version),
                "loader": optional_filter(&self.modpack_browser.loader),
            }),
            json!({"provider": provider, "generation": generation}),
        );
    }

    fn modpack_browser(&mut self, ui: &mut egui::Ui) {
        let mut search = false;
        let mut import = false;
        let spacing = ui.spacing().item_spacing.x;
        let busy = self.modpack_browser.searching || self.modpack.analyzing;
        let busy_width = if busy {
            ACTION_BUTTON_SIZE.x + spacing
        } else {
            0.0
        };
        let fields_width =
            (ui.available_width() - ACTION_BUTTON_SIZE.x * 2.0 - spacing * 4.0 - busy_width)
                .max(0.0);
        let filter_width = (fields_width / 3.0).min(FILTER_COMBO_WIDTH);
        let search_width = (fields_width - filter_width * 2.0).max(0.0);
        let search_hint = self.tr("create.search_hint");
        ui.horizontal(|ui| {
            ui.spacing_mut().interact_size.y = ACTION_BUTTON_SIZE.y;
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.modpack_browser.query)
                    .desired_width(search_width)
                    .min_size(egui::vec2(search_width, ACTION_BUTTON_SIZE.y))
                    .hint_text(search_hint),
            );
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                search = true;
            }
            if action_icon_button(ui, ActionIcon::Search, &self.tr("create.search"), true).clicked()
            {
                search = true;
            }
            self.modpack_game_filter(ui, filter_width, &mut search);
            self.modpack_loader_filter(ui, filter_width, &mut search);
            if action_icon_button(
                ui,
                ActionIcon::Import,
                &self.tr("action.import_modpack"),
                !self.modpack.analyzing && !self.modpack.importing,
            )
            .clicked()
            {
                import = true;
            }
            if busy {
                ui.add_sized(ACTION_BUTTON_SIZE, egui::Spinner::new());
            }
        });
        if search {
            self.search_modpacks();
        }
        if import {
            self.begin_modpack_import();
        }
        self.local_import_preview(ui);
        ui.add_space(crate::layout::CARD_LIST_GAP);

        let results = self.modpack_browser.results.clone();
        let requested = self.modpack_browser.requested.clone();
        let mut install = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if results.is_empty() && !self.modpack_browser.searching {
                    ui.label(self.tr("create.modpack_empty"));
                }
                for project in results {
                    if let Some(installed) = modpack_row(ui, &project, &requested, self) {
                        install = Some(installed);
                    }
                    ui.add_space(crate::layout::CARD_LIST_GAP);
                }
            });
        if let Some(project) = install {
            self.install_modpack(project);
        }
    }

    fn modpack_game_filter(&mut self, ui: &mut egui::Ui, width: f32, changed: &mut bool) {
        let selected = if self.modpack_browser.game_version.is_empty() {
            self.tr("create.all_versions")
        } else {
            self.modpack_browser.game_version.clone()
        };
        egui::ComboBox::from_id_salt(("new_instance_game_filter", &self.modpack_browser.provider))
            .width(width)
            .truncate()
            .selected_text(selected)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(
                        self.modpack_browser.game_version.is_empty(),
                        self.tr("create.all_versions"),
                    )
                    .clicked()
                {
                    self.modpack_browser.game_version.clear();
                    *changed = true;
                }
                for version in self.versions.clone() {
                    if ui
                        .selectable_label(
                            self.modpack_browser.game_version == version.id,
                            &version.id,
                        )
                        .clicked()
                    {
                        self.modpack_browser.game_version = version.id;
                        *changed = true;
                    }
                }
            });
    }

    fn modpack_loader_filter(&mut self, ui: &mut egui::Ui, width: f32, changed: &mut bool) {
        let selected = if self.modpack_browser.loader.is_empty() {
            self.tr("create.all_loaders")
        } else {
            self.tr(loader_key(&self.modpack_browser.loader))
        };
        egui::ComboBox::from_id_salt((
            "new_instance_loader_filter",
            &self.modpack_browser.provider,
        ))
        .width(width)
        .truncate()
        .selected_text(selected)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(
                    self.modpack_browser.loader.is_empty(),
                    self.tr("create.all_loaders"),
                )
                .clicked()
            {
                self.modpack_browser.loader.clear();
                *changed = true;
            }
            for (value, key) in [
                ("fabric", "loader.fabric"),
                ("forge", "loader.forge"),
                ("neo_forge", "loader.neo_forge"),
            ] {
                if ui
                    .selectable_label(self.modpack_browser.loader == value, self.tr(key))
                    .clicked()
                {
                    self.modpack_browser.loader = value.into();
                    *changed = true;
                }
            }
        });
    }

    fn install_modpack(&mut self, project: ModProject) {
        self.modpack_browser
            .requested
            .insert(project.project_id.clone());
        self.call(
            "modpack.install",
            json!({
                "provider": self.modpack_browser.provider,
                "projectId": project.project_id,
                "projectName": project.name,
                "gameVersion": optional_filter(&self.modpack_browser.game_version),
                "loader": optional_filter(&self.modpack_browser.loader),
                "name": Value::Null,
            }),
            json!({"projectId": project.project_id}),
        );
    }

    fn begin_modpack_import(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Modpack", &["zip", "mrpack"])
            .pick_file()
        else {
            return;
        };
        self.modpack.path = path.display().to_string();
        self.modpack.name.clear();
        self.modpack.analysis = None;
        self.modpack.analyzing = true;
        self.modpack.importing = false;
        self.call(
            "modpack.analyze",
            json!({"path": self.modpack.path.trim()}),
            Value::Null,
        );
    }

    fn local_import_preview(&mut self, ui: &mut egui::Ui) {
        let Some(analysis) = self.modpack.analysis.clone() else {
            return;
        };
        let mut import = false;
        ui.add_space(crate::layout::CARD_LIST_GAP);
        crate::card_frame(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.strong(&analysis.name);
                if let Some(version) = &analysis.version {
                    ui.label(version);
                }
            });
            ui.label(format!(
                "{} · {} {}",
                analysis.game_version,
                self.tr(loader_key(&analysis.loader)),
                analysis.loader_version.as_deref().unwrap_or_default()
            ));
            for warning in &analysis.warnings {
                ui.add(egui::Label::new(egui::RichText::new(warning).small()).wrap());
            }
            ui.horizontal(|ui| {
                ui.label(self.tr("modpack.instance_name"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.modpack.name)
                        .desired_width(ui.available_width()),
                );
            });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.modpack.importing,
                        egui::Button::new(self.tr("modpack.import")),
                    )
                    .clicked()
                {
                    import = true;
                }
                if self.modpack.importing {
                    ui.spinner();
                }
            });
        });
        if import {
            self.modpack.importing = true;
            self.call(
                "modpack.import",
                json!({
                    "path": self.modpack.path.trim(),
                    "name": if self.modpack.name.trim().is_empty() {
                        Value::Null
                    } else {
                        json!(self.modpack.name.trim())
                    },
                }),
                Value::Null,
            );
        }
    }

    fn custom_instance_form(&mut self, ui: &mut egui::Ui) {
        let Some(mut form) = self.create.clone() else {
            return;
        };
        let previous_version = form.game_version.clone();
        let previous_loader = form.loader.clone();
        let mut submit = false;
        ui.add_space(crate::layout::CARD_LIST_GAP);
        egui::Grid::new("custom_instance_form")
            .num_columns(4)
            .spacing([crate::layout::ITEM_SPACING.x, crate::layout::SECTION_GAP])
            .show(ui, |ui| {
                ui.label(self.tr("create.game_version"));
                let selected = if form.game_version.is_empty() {
                    self.tr("common.select")
                } else {
                    form.game_version.clone()
                };
                egui::ComboBox::from_id_salt("custom_game_version")
                    .width(CUSTOM_FIELD_WIDTH)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for version in self.versions.clone() {
                            ui.selectable_value(
                                &mut form.game_version,
                                version.id.clone(),
                                format!("{} ({})", version.id, version.release_type),
                            );
                        }
                    });
                ui.checkbox(
                    &mut form.include_snapshots,
                    self.tr("create.include_snapshots"),
                );
                ui.end_row();

                ui.label(self.tr("create.loader"));
                egui::ComboBox::from_id_salt("custom_loader")
                    .width(CUSTOM_FIELD_WIDTH)
                    .selected_text(self.tr(loader_key(&form.loader)))
                    .show_ui(ui, |ui| {
                        for (value, key) in [
                            ("vanilla", "loader.vanilla"),
                            ("fabric", "loader.fabric"),
                            ("forge", "loader.forge"),
                            ("neo_forge", "loader.neo_forge"),
                        ] {
                            ui.selectable_value(&mut form.loader, value.to_owned(), self.tr(key));
                        }
                    });
                if form.loader == "vanilla" {
                    ui.end_row();
                } else {
                    ui.label(self.tr("create.loader_version"));
                    let selected = if form.loader_version.is_empty() {
                        self.tr("common.select")
                    } else {
                        form.loader_version.clone()
                    };
                    egui::ComboBox::from_id_salt("custom_loader_version")
                        .width(CUSTOM_FIELD_WIDTH)
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for version in self.loader_versions.clone() {
                                ui.selectable_value(
                                    &mut form.loader_version,
                                    version.version.clone(),
                                    format!(
                                        "{}{}",
                                        version.version,
                                        if version.stable {
                                            self.tr("create.stable_suffix")
                                        } else {
                                            String::new()
                                        }
                                    ),
                                );
                            }
                        });
                    ui.end_row();
                }
            });
        ui.add_space(crate::layout::SECTION_GAP);
        ui.horizontal(|ui| {
            let valid = !form.game_version.is_empty()
                && (form.loader == "vanilla" || !form.loader_version.is_empty())
                && !form.creating;
            if action_icon_button(
                ui,
                ActionIcon::Play,
                &self.tr("create.create_install"),
                valid,
            )
            .clicked()
            {
                submit = true;
            }
            if form.creating {
                ui.spinner();
            }
        });

        let snapshots_changed = self
            .create
            .as_ref()
            .is_some_and(|current| current.include_snapshots != form.include_snapshots);
        let version_changed = previous_version != form.game_version;
        let loader_changed = previous_loader != form.loader;
        if version_changed || loader_changed {
            form.loader_version.clear();
            self.loader_versions.clear();
        }
        form.creating = form.creating || submit;
        if submit {
            form.name = if form.loader == "vanilla" {
                format!("Minecraft {}", form.game_version)
            } else {
                format!(
                    "{} {}",
                    form.game_version,
                    self.tr(loader_key(&form.loader))
                )
            };
        }
        self.create = Some(form.clone());
        if snapshots_changed {
            self.versions.clear();
            self.call(
                "version.list",
                json!({"includeSnapshots": form.include_snapshots}),
                json!({"includeSnapshots": form.include_snapshots}),
            );
        }
        if (version_changed || loader_changed)
            && form.loader != "vanilla"
            && !form.game_version.is_empty()
        {
            self.call(
                "loader.list",
                json!({"gameVersion": form.game_version, "loader": form.loader}),
                json!({"gameVersion": form.game_version, "loader": form.loader}),
            );
        }
        if submit {
            self.call(
                "instance.create",
                json!({
                    "name": form.name.trim(),
                    "gameVersion": form.game_version,
                    "loader": form.loader,
                    "loaderVersion": if form.loader == "vanilla" {
                        Value::Null
                    } else {
                        json!(form.loader_version)
                    },
                    "javaPath": Value::Null,
                    "minMemoryMb": Value::Null,
                    "maxMemoryMb": Value::Null,
                    "jvmArgs": Value::Null,
                    "gameArgs": [],
                    "accountId": Value::Null,
                }),
                json!({"installAfterCreate": true}),
            );
        }
    }
}

fn modpack_row(
    ui: &mut egui::Ui,
    project: &ModProject,
    requested: &std::collections::HashSet<String>,
    app: &MzApp,
) -> Option<ModProject> {
    let mut install = None;
    crate::card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            modpack_icon(ui, project);
            let spacing = ui.spacing().item_spacing.x;
            let info_width = (ui.available_width() - ACTION_BUTTON_SIZE.x - spacing).max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(info_width, MODPACK_ROW_HEIGHT),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&project.name).size(16.0).strong())
                            .truncate(),
                    );
                    let versions = if project.game_versions.is_empty() {
                        "—".into()
                    } else {
                        project.game_versions.join(", ")
                    };
                    let loaders = project
                        .loaders
                        .iter()
                        .map(|loader| app.tr(loader_key(loader)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    ui.add(egui::Label::new(format!("{versions} · {loaders}")).truncate());
                    ui.add(egui::Label::new(&project.summary).truncate());
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ACTION_BUTTON_SIZE.x, MODPACK_ROW_HEIGHT),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if requested.contains(&project.project_id) {
                        ui.spinner();
                    } else if action_icon_button(
                        ui,
                        ActionIcon::Download,
                        &app.tr("action.install"),
                        true,
                    )
                    .clicked()
                    {
                        install = Some(project.clone());
                    }
                },
            );
        });
    });
    install
}

fn modpack_icon(ui: &mut egui::Ui, project: &ModProject) {
    let (rect, _) = ui.allocate_exact_size(MODPACK_ICON_SIZE, egui::Sense::hover());
    let color = match project.provider.as_str() {
        "curse_forge" => egui::Color32::from_rgb(121, 82, 179),
        "ftb" => egui::Color32::from_rgb(45, 128, 151),
        _ => egui::Color32::from_rgb(74, 157, 93),
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(8),
        color,
        egui::Stroke::NONE,
        egui::StrokeKind::Inside,
    );
    let initial = project.name.chars().next().unwrap_or('M').to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(24.0),
        egui::Color32::WHITE,
    );
}

#[derive(Clone, Copy)]
enum ActionIcon {
    Download,
    Import,
    Play,
    Search,
}

fn action_icon_button(
    ui: &mut egui::Ui,
    icon: ActionIcon,
    tooltip: &str,
    enabled: bool,
) -> egui::Response {
    let (name, theme, bytes) = match (icon, ui.visuals().dark_mode) {
        (ActionIcon::Download, false) => (
            "download",
            "light",
            &include_bytes!("../../assets/download-light.png")[..],
        ),
        (ActionIcon::Download, true) => (
            "download",
            "dark",
            &include_bytes!("../../assets/download-dark.png")[..],
        ),
        (ActionIcon::Import, false) => (
            "import",
            "light",
            &include_bytes!("../../assets/import-light.png")[..],
        ),
        (ActionIcon::Import, true) => (
            "import",
            "dark",
            &include_bytes!("../../assets/import-dark.png")[..],
        ),
        (ActionIcon::Play, false) => (
            "play",
            "light",
            &include_bytes!("../../assets/play-light.png")[..],
        ),
        (ActionIcon::Play, true) => (
            "play",
            "dark",
            &include_bytes!("../../assets/play-dark.png")[..],
        ),
        (ActionIcon::Search, false) => (
            "search",
            "light",
            &include_bytes!("../../assets/search-light.png")[..],
        ),
        (ActionIcon::Search, true) => (
            "search",
            "dark",
            &include_bytes!("../../assets/search-dark.png")[..],
        ),
    };
    let image = egui::Image::from_bytes(format!("bytes://new-instance-{name}-{theme}.png"), bytes)
        .fit_to_exact_size(egui::Vec2::splat(18.0));
    ui.add_enabled(
        enabled,
        egui::Button::image(image)
            .min_size(ACTION_BUTTON_SIZE)
            .corner_radius(egui::CornerRadius::same(7)),
    )
    .on_hover_text(tooltip)
}

fn optional_filter(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    }
}
