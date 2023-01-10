use rustfft::{num_complex::Complex, FftPlanner};
use std::{collections::VecDeque, sync::Mutex};
use wavegen::{sawtooth, sine, square, PeriodicFunction, Waveform};

static FFT_PLANNER: once_cell::sync::Lazy<Mutex<FftPlanner<f64>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(FftPlanner::new()));

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct Main {
    sample_rate: f64,
    n_samples: u16,
    components: Vec<ComponentWrapper>,

    #[serde(skip)]
    frames: u128,

    #[serde(skip)]
    history: History,
}

impl Default for Main {
    fn default() -> Self {
        Self {
            sample_rate: 3000.0,
            n_samples: 1000,
            components: vec![],
            frames: 0,
            history: History::new(),
        }
    }
}

impl Main {
    /// Called once before the first frame.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            return eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default();
        }

        Self::default()
    }
}

impl eframe::App for Main {
    /// Called by the frame work to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    #[allow(clippy::too_many_lines)]
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let Self {
            sample_rate,
            n_samples,
            components,
            frames,
            history,
        } = self;

        history.on_new_frame(ctx.input().time, frame.info().cpu_usage);
        *frames += 1;

        #[cfg(not(target_arch = "wasm32"))] // no File->Quit on web pages!
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        frame.close();
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                egui::warn_if_debug_build(ui);
                ui.hyperlink_to(
                    egui::RichText::new(format!("Version: git:{}", env!("GIT_HASH"))).small(),
                    format!(
                        "https://github.com/spitfire05/egui-waves/commit/{}",
                        env!("GIT_HASH")
                    ),
                );
                ui.label(egui::RichText::new(format!("Total frames painted: {frames}")).small());
                ui.label(
                    egui::RichText::new(format!("Mean CPU usage: {:.2} ms", history.mean_ms()))
                        .small(),
                )
                .on_hover_ui(|ui| history.show_plot(ui));
            });
        });

        egui::SidePanel::left("left_panel").show(ctx, |ui| {
            ui.heading("Add new component");
            if ui.button("Sine").clicked() {
                components.push(ComponentWrapper {
                    inner: Component::Sine {
                        frequency: 100.0,
                        amplitude: 1.0,
                        phase: 0.0,
                    },
                    name: "Sine".to_string(),
                    enabled: true,
                });
            }

            if ui.button("Square").clicked() {
                components.push(ComponentWrapper {
                    inner: Component::Square {
                        frequency: 100.0,
                        amplitude: 1.0,
                        phase: 0.0,
                    },
                    name: "Square".to_string(),
                    enabled: true,
                });
            }

            if ui.button("Sawtooth").clicked() {
                components.push(ComponentWrapper {
                    inner: Component::Sawtooth {
                        frequency: 100.0,
                        amplitude: 1.0,
                        phase: 0.0,
                    },
                    name: "Sawtooth".to_string(),
                    enabled: true,
                });
            }

            egui::Frame::none().show(ui, |ui| {
                ui.heading("Settings");
                ui.add(
                    egui::DragValue::new(sample_rate)
                        .clamp_range(f64::MIN_POSITIVE..=f64::MAX)
                        .prefix("Sample rate: ")
                        .suffix(" Hz"),
                );
                ui.add(
                    egui::DragValue::new(n_samples)
                        .clamp_range(usize::MIN..=usize::MAX)
                        .prefix("N Samples: "),
                );
            })
        });

        egui::SidePanel::right("right_panel").show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for c in components.iter_mut().filter(|c| c.enabled) {
                    egui::Frame::none()
                        .fill(ui.visuals().faint_bg_color)
                        .outer_margin(10.0)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                c.show(ui);
                                if ui.button("❌ Remove").clicked() {
                                    c.enabled = false;
                                }
                            });
                        });
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's

            ui.heading("Plot");

            let waveform = Waveform::<f64, f64>::with_components(
                *sample_rate,
                components.iter().map(|c| c.inner.build()).collect(),
            );

            #[allow(clippy::cast_precision_loss)]
            let points: egui::plot::PlotPoints = waveform
                .into_iter()
                .enumerate()
                .map(|(i, x)| [i as f64 / *sample_rate, x])
                .take(*n_samples as usize)
                .collect();
            let line = egui::plot::Line::new(points);
            egui::plot::Plot::new("wf_plot")
                .view_aspect(3.0)
                .show(ui, |plot_ui| plot_ui.line(line));

            ui.heading("Spectrum");

            let fft = FFT_PLANNER
                .lock()
                .expect("Could not get lock on FFT_PLANNER")
                .plan_fft_forward(*n_samples as usize);

            let mut buffer: Vec<_> = waveform
                .into_iter()
                .map(|s| Complex::new(s, 0.0))
                .take(*n_samples as usize)
                .collect();
            fft.process(&mut buffer);
            let fmax = *sample_rate / 2.56;
            let spectrum_resolution = *sample_rate / f64::from(*n_samples);
            #[allow(clippy::cast_precision_loss)]
            let points: egui::plot::PlotPoints = buffer
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    [
                        i as f64 * spectrum_resolution,
                        c.norm() / f64::from(*n_samples),
                    ]
                })
                .take_while(|[f, _]| *f < fmax)
                .collect();
            let line = egui::plot::Line::new(points);
            egui::plot::Plot::new("spectrum_plot")
                .view_aspect(4.0)
                .legend(egui::plot::Legend::default())
                .show(ui, |plot_ui| {
                    plot_ui.line(line);
                    for c in components.iter() {
                        plot_ui.vline(
                            egui::plot::VLine::new(c.inner.frequency()).name(c.name.clone()),
                        );
                    }
                });
        });

        while let Some(i) = components.iter().position(|c| !c.enabled) {
            components.remove(i);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ComponentWrapper {
    inner: Component,
    name: String,
    enabled: bool,
}

impl ComponentWrapper {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = ui.label("Name: ");
            ui.text_edit_singleline(&mut self.name)
                .labelled_by(label.id).on_hover_text("Name of this component.\n\
                                                      This is currently only used for spectrum marker");
        });
        ui.vertical(|ui| {
            self.inner.show(ui);
        });
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
enum Component {
    Sine {
        frequency: f64,
        amplitude: f64,
        phase: f64,
    },
    Square {
        frequency: f64,
        amplitude: f64,
        phase: f64,
    },
    Sawtooth {
        frequency: f64,
        amplitude: f64,
        phase: f64,
    },
}

impl Component {
    pub fn build(&self) -> PeriodicFunction<f64> {
        match self {
            Component::Sine {
                frequency,
                amplitude,
                phase,
            } => sine!(*frequency, *amplitude, *phase),
            Component::Square {
                frequency,
                amplitude,
                phase,
            } => square!(*frequency, *amplitude, *phase),
            Component::Sawtooth {
                frequency,
                amplitude,
                phase,
            } => sawtooth!(*frequency, *amplitude, *phase),
        }
    }

    pub fn frequency(&self) -> f64 {
        match self {
            Component::Square {
                frequency,
                amplitude: _,
                phase: _,
            }
            | Component::Sine {
                frequency,
                amplitude: _,
                phase: _,
            }
            | Component::Sawtooth {
                frequency,
                amplitude: _,
                phase: _,
            } => *frequency,
        }
    }

    fn show_control(
        ui: &mut egui::Ui,
        name: impl Into<String>,
        frequency: &mut f64,
        amplitude: &mut f64,
        phase: &mut f64,
    ) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(name).strong());
            ui.add(
                egui::DragValue::new(frequency)
                    .clamp_range(1e-2..=f64::MAX)
                    .prefix("f: ")
                    .suffix(" Hz"),
            );
            ui.add(
                egui::DragValue::new(amplitude)
                    .clamp_range(0.0..=f64::MAX)
                    .prefix("A: "),
            );
            ui.add(egui::Slider::new(phase, 0.0..=1.0).prefix("φ: "));
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        match self {
            Component::Sine {
                frequency,
                amplitude,
                phase,
            } => Self::show_control(ui, "Sine", frequency, amplitude, phase),
            Component::Square {
                frequency,
                amplitude,
                phase,
            } => Self::show_control(ui, "Square", frequency, amplitude, phase),
            Component::Sawtooth {
                frequency,
                amplitude,
                phase,
            } => Self::show_control(ui, "Sawtooth", frequency, amplitude, phase),
        };
    }
}

const HISTORY_SIZE: usize = 1024;

struct History {
    frame_times: VecDeque<f32>,
}

impl History {
    pub fn new() -> Self {
        History {
            frame_times: VecDeque::with_capacity(HISTORY_SIZE),
        }
    }

    pub fn mean_ms(&self) -> f32 {
        let n = self.frame_times.len();

        #[allow(clippy::cast_precision_loss)]
        self.frame_times
            .iter()
            .map(|x| *x * 1000.0)
            .fold(0.0, |acc, x| (acc + x / (n as f32)))
    }

    pub fn on_new_frame(&mut self, now: f64, previous_frame_time: Option<f32>) {
        while self.frame_times.len() >= HISTORY_SIZE {
            self.frame_times.pop_front();
        }

        if let Some(t) = previous_frame_time {
            self.frame_times.push_back(t);
        }
    }

    pub fn show_plot(&self, ui: &mut egui::Ui) {
        let points: egui::plot::PlotPoints = self
            .frame_times
            .iter()
            .enumerate()
            .map(|(i, x)| [i as f64, (*x as f64) * 1000.0])
            .collect();
        let line = egui::plot::Line::new(points);
        egui::plot::Plot::new("frame_history_plot")
            .view_aspect(3.0)
            .show(ui, |plot_ui| plot_ui.line(line));
    }
}
