use crate::cli::Options;
use eframe::egui;
use rubblekin_launcher::{InstalledClient, Launcher, Progress};
use std::{sync::mpsc, time::Duration};

enum Event {
    Progress(Progress),
    Failed {
        error: String,
        cached: Option<InstalledClient>,
    },
    Launched,
}

struct Window {
    options: Options,
    events: Option<mpsc::Receiver<Event>>,
    progress: Progress,
    error: Option<String>,
    cached: Option<InstalledClient>,
}

impl Window {
    fn start(&mut self, context: egui::Context, cached: Option<InstalledClient>) {
        let (sender, receiver) = mpsc::channel();
        self.events = Some(receiver);
        self.error = None;
        self.cached = None;
        self.progress = Progress::Checking;
        let data_dir = self.options.data_dir.clone();
        let args = self.options.client_args.clone();
        let offline = self.options.offline;
        std::thread::spawn(move || {
            let send = |event| {
                let _ = sender.send(event);
                context.request_repaint();
            };
            let launcher = match Launcher::open(data_dir) {
                Ok(launcher) => launcher,
                Err(error) => {
                    send(Event::Failed {
                        error,
                        cached: None,
                    });
                    return;
                }
            };
            let result = if let Some(client) = cached {
                Ok(client)
            } else if offline {
                launcher.cached().and_then(|client| {
                    client.ok_or_else(|| "No client is installed yet. Connect to the internet for the first download.".into())
                })
            } else {
                launcher.update(|progress| send(Event::Progress(progress)))
            };
            let result = result.and_then(|client| client.launch(&args));
            match result {
                Ok(_) => send(Event::Launched),
                Err(error) => send(Event::Failed {
                    error,
                    cached: launcher.cached().ok().flatten(),
                }),
            }
        });
    }

    fn receive(&mut self, context: &egui::Context) {
        let Some(events) = &self.events else { return };
        let mut finished = false;
        loop {
            match events.try_recv() {
                Ok(Event::Progress(progress)) => self.progress = progress,
                Ok(Event::Launched) => {
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                    finished = true;
                    break;
                }
                Ok(Event::Failed { error, cached }) => {
                    self.error = Some(error);
                    self.cached = cached;
                    finished = true;
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error =
                        Some("The update worker stopped unexpectedly. Please retry.".into());
                    finished = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if finished {
            self.events = None;
        }
    }
}

impl eframe::App for Window {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(24.0))
            .show(ui, |ui| {
                ui.heading(egui::RichText::new("Rubblekin").size(30.0));
                ui.label("A living voxel valley");
                ui.add_space(24.0);
                if let Some(error) = &self.error {
                    ui.label(egui::RichText::new("Couldn't start the latest client").size(19.0));
                    ui.add_space(8.0);
                    // Keep the concrete failure visible, including first-run
                    // offline errors, missing releases, and disk failures.
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .show(ui, |ui| {
                            ui.label(error);
                        });
                    ui.add_space(16.0);
                    let mut retry = false;
                    let mut play_cached = false;
                    ui.horizontal_wrapped(|ui| {
                        retry = ui.button("Retry").clicked();
                        if let Some(client) = &self.cached {
                            play_cached = ui
                                .button("Play installed version")
                                .on_hover_text(format!("Client {}", &client.commit[..12]))
                                .clicked();
                        }
                        if ui.button("Close").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    if retry {
                        self.start(ui.ctx().clone(), None);
                    } else if play_cached {
                        let cached = self.cached.take();
                        self.start(ui.ctx().clone(), cached);
                    }
                } else {
                    let status = match &self.progress {
                        Progress::Checking => "Checking for updates…",
                        Progress::Downloading { .. } => "Downloading the latest client…",
                        Progress::Verifying => "Verifying the download…",
                        Progress::Installing => "Installing the client…",
                    };
                    ui.label(egui::RichText::new(status).size(19.0));
                    ui.add_space(14.0);
                    if let Progress::Downloading { downloaded, total } = self.progress {
                        let downloaded_mb = downloaded as f64 / 1_048_576.0;
                        if let Some(total) = total.filter(|total| *total > 0) {
                            ui.add(
                                egui::ProgressBar::new(downloaded as f32 / total as f32).text(
                                    format!(
                                        "{downloaded_mb:.1} / {:.1} MB",
                                        total as f64 / 1_048_576.0
                                    ),
                                ),
                            );
                        } else {
                            ui.spinner();
                            ui.label(format!("{downloaded_mb:.1} MB downloaded"));
                        }
                    } else {
                        ui.spinner();
                    }
                    ui.add_space(20.0);
                    ui.label("The game will open automatically when it's ready.");
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            });
    }
}

pub fn run(options: Options) -> Result<(), String> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 420.0])
            .with_min_inner_size([480.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Rubblekin Launcher",
        native_options,
        Box::new(move |creation| {
            let mut fonts = egui::FontDefinitions::empty();
            fonts.font_data.insert(
                "Atkinson".into(),
                egui::FontData::from_static(include_bytes!(
                    "../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf"
                ))
                .into(),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.insert(family, vec!["Atkinson".into()]);
            }
            creation.egui_ctx.set_fonts(fonts);
            creation.egui_ctx.all_styles_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, egui::FontId::proportional(17.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(17.0));
                style.spacing.button_padding = egui::vec2(12.0, 8.0);
            });
            let mut window = Window {
                options,
                events: None,
                progress: Progress::Checking,
                error: None,
                cached: None,
            };
            window.start(creation.egui_ctx.clone(), None);
            Ok(Box::new(window))
        }),
    )
    .map_err(|error| {
        format!("Cannot open the launcher window: {error}. Try --headless from a terminal.")
    })
}
