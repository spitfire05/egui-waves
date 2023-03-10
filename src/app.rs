use crate::util::{Cache, PlotData};
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::Mutex;
use wavegen::{sawtooth, sine, square, PeriodicFunction, Waveform};

const FMAX_SCALE: f64 = 2.56;

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
    history: History,

    #[serde(skip)]
    plot_data_cache: Cache<PlotData>,
}

impl Default for Main {
    fn default() -> Self {
        Self {
            sample_rate: 3000.0,
            n_samples: 1000,
            components: vec![],
            history: History::new(),
            plot_data_cache: Cache::default(),
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
            history,
            plot_data_cache,
        } = self;

        history.on_new_frame(ctx.input().time, frame.info().cpu_usage);

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
                if cfg!(debug_assertions) {
                    ui.separator();
                }
                ui.hyperlink_to(
                    egui::RichText::new(format!("Version: git:{}", env!("GIT_HASH"))).small(),
                    format!(
                        "https://github.com/spitfire05/egui-waves/commit/{}",
                        env!("GIT_HASH")
                    ),
                );
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("Total frames painted: {}", history.total()))
                        .small(),
                );
                ui.separator();
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
                plot_data_cache.invalidate();
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
                plot_data_cache.invalidate();
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
                plot_data_cache.invalidate();
            }

            ui.separator();

            ui.heading("Settings");
            if ui
                .add(
                    egui::DragValue::new(sample_rate)
                        .clamp_range(f64::MIN_POSITIVE..=f64::MAX)
                        .prefix("Sample rate: ")
                        .suffix(" Hz"),
                )
                .changed()
            {
                plot_data_cache.invalidate();
            }
            if ui
                .add(
                    egui::DragValue::new(n_samples)
                        .clamp_range(usize::MIN..=usize::MAX)
                        .prefix("N Samples: "),
                )
                .changed()
            {
                plot_data_cache.invalidate();
            }
        });

        egui::SidePanel::right("right_panel").show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for c in components.iter_mut().filter(|c| c.enabled) {
                    egui::Frame::none()
                        .fill(ui.visuals().faint_bg_color)
                        .outer_margin(10.0)
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                c.show(ui, *sample_rate, plot_data_cache);
                            });
                        });
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's

            ui.heading("Plot");

            let pd = plot_data_cache.get_or_init(|| {
                let waveform: Vec<_> = Waveform::<f64, f64>::with_components(
                    *sample_rate,
                    components.iter().map(|c| c.inner.build()).collect(),
                )
                .iter()
                .take(*n_samples as usize)
                .collect();

                PlotData {
                    waveform: {
                        waveform
                            .iter()
                            .enumerate()
                            .map(|(i, x)| [i as f64 / *sample_rate, *x])
                            .collect()
                    },
                    spectrum: {
                        let fmax = *sample_rate / FMAX_SCALE;
                        let spectrum_resolution = *sample_rate / f64::from(*n_samples);
                        let mut buffer: Vec<_> =
                            waveform.into_iter().map(|s| Complex::new(s, 0.0)).collect();
                        let fft = FFT_PLANNER
                            .lock()
                            .expect("Could not get lock on FFT_PLANNER")
                            .plan_fft_forward(*n_samples as usize);
                        fft.process(&mut buffer);
                        buffer
                            .iter()
                            .enumerate()
                            .map(|(i, c)| {
                                [
                                    i as f64 * spectrum_resolution,
                                    c.norm() / f64::from(*n_samples),
                                ]
                            })
                            .take_while(|[f, _]| *f < fmax)
                            .collect()
                    },
                }
            });

            #[allow(clippy::cast_precision_loss)]
            let points = egui::plot::PlotPoints::from(pd.waveform.clone());
            let line = egui::plot::Line::new(points);
            egui::plot::Plot::new("wf_plot")
                .view_aspect(4.0)
                .show(ui, |plot_ui| plot_ui.line(line));

            ui.heading("Spectrum");

            #[allow(clippy::cast_precision_loss)]
            let points = egui::plot::PlotPoints::from(pd.spectrum.clone());
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
            plot_data_cache.invalidate();
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
    pub fn show<T>(&mut self, ui: &mut egui::Ui, sampling_frequency: f64, cache: &mut Cache<T>) {
        ui.horizontal(|ui| {
            let label = ui.label("Name: ");
            ui.text_edit_singleline(&mut self.name)
                .labelled_by(label.id).on_hover_text("Name of this component.\n\
                                                      This is currently only used for spectrum marker");
        });
        ui.vertical(|ui| {
            self.inner.show(ui, cache);
            if self.inner.frequency() * FMAX_SCALE > sampling_frequency {
                ui.label(
                    egui::RichText::new("??? Above Nyquist frequency ???")
                        .color(ui.visuals().warn_fg_color),
                );
            }
            if ui.button("??? Remove").clicked() {
                self.enabled = false;
            }
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

    fn show_control<T>(
        ui: &mut egui::Ui,
        name: impl Into<String>,
        frequency: &mut f64,
        amplitude: &mut f64,
        phase: &mut f64,
        cache: &mut Cache<T>,
    ) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(name).strong());
            if ui
                .add(
                    egui::DragValue::new(frequency)
                        .clamp_range(1e-2..=f64::MAX)
                        .prefix("f: ")
                        .suffix(" Hz"),
                )
                .changed()
                || ui
                    .add(
                        egui::DragValue::new(amplitude)
                            .clamp_range(0.0..=f64::MAX)
                            .prefix("A: "),
                    )
                    .changed()
                || ui
                    .add(egui::Slider::new(phase, 0.0..=1.0).prefix("??: "))
                    .changed()
            {
                cache.invalidate();
            }
        });
    }

    pub fn show<T>(&mut self, ui: &mut egui::Ui, cache: &mut Cache<T>) {
        match self {
            Component::Sine {
                frequency,
                amplitude,
                phase,
            } => Self::show_control(ui, "Sine", frequency, amplitude, phase, cache),
            Component::Square {
                frequency,
                amplitude,
                phase,
            } => Self::show_control(ui, "Square", frequency, amplitude, phase, cache),
            Component::Sawtooth {
                frequency,
                amplitude,
                phase,
            } => Self::show_control(ui, "Sawtooth", frequency, amplitude, phase, cache),
        };
    }
}

const HISTORY_SIZE: usize = 1024;
const MAX_HISTORY_AGE: f32 = 1.0;

struct History {
    frame_times: egui::util::History<f32>,
}

impl History {
    pub fn new() -> Self {
        History {
            frame_times: egui::util::History::new(0..HISTORY_SIZE, MAX_HISTORY_AGE),
        }
    }

    pub fn total(&self) -> u64 {
        self.frame_times.total_count()
    }

    pub fn mean_ms(&self) -> f32 {
        self.frame_times.average().unwrap_or_default() * 1000.0
    }

    pub fn on_new_frame(&mut self, now: f64, previous_frame_time: Option<f32>) {
        let previous_frame_time = previous_frame_time.unwrap_or_default();
        if let Some(latest) = self.frame_times.latest_mut() {
            *latest = previous_frame_time; // rewrite history now that we know
        }
        self.frame_times.add(now, previous_frame_time); // projected
    }

    pub fn show_plot(&self, ui: &mut egui::Ui) {
        #[allow(clippy::cast_precision_loss)]
        let points: egui::plot::PlotPoints = self
            .frame_times
            .iter()
            .enumerate()
            .map(|(i, (_, x))| [i as f64, f64::from(x) * 1000.0])
            .collect();
        let line = egui::plot::Line::new(points);
        egui::plot::Plot::new("frame_history_plot")
            .view_aspect(3.0)
            .show(ui, |plot_ui| plot_ui.line(line));
    }
}
