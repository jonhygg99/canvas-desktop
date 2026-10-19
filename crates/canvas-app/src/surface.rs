//! Textura offscreen donde vello pinta el lienzo, registrada en egui para
//! poder mostrarla como imagen.

use canvas_render::{CanvasRenderer, RenderError};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use vello::wgpu;

/// Cortacircuitos del render vello: si la GPU escupe fallos seguidos (el
/// preludio del crash de hoy — `Dropped frame: Validation` y luego el
/// pánico del staging belt de egui-wgpu, fuera de nuestro código), se deja
/// de enviarle trabajo para no envenenar el device/queue compartido con
/// egui. El lienzo se congela en la última textura buena y la barra de
/// estado avisa; el usuario puede guardar (ya con sus propios avisos) y
/// reiniciar. El corte se rearma solo: cada ~5 s se deja pasar un render
/// de sonda y, si la GPU responde, se reanuda (un hipo transitorio del
/// driver no congela el lienzo para toda la sesión). La recreación de la
/// superficie (resize) también lo rearma. Puro y testeable: no toca GPU.
struct GpuHealth {
    streak: u32,
    dead: bool,
    /// Frames con el corte disparado desde la última sonda.
    dead_frames: u32,
}

/// Fallos de render seguidos que disparan el corte (~0,5 s a 60 fps:
/// un hipo suelto del driver no congela el lienzo, una GPU muerta sí).
const GPU_FAIL_TRIP: u32 = 30;
/// Frames con el corte disparado entre sondas (~5 s a 60 fps: la GPU
/// enferma no recibe trabajo, pero una recuperada se detecta pronto).
const GPU_PROBE_EVERY: u32 = 300;

impl GpuHealth {
    fn record(&mut self, ok: bool) {
        if self.dead {
            return;
        }
        if ok {
            self.streak = 0;
        } else {
            self.streak += 1;
            if self.streak >= GPU_FAIL_TRIP {
                self.dead = true;
                self.dead_frames = 0;
            }
        }
    }

    /// ¿Toca una sonda? Solo con el corte disparado, cada `GPU_PROBE_EVERY`
    /// frames: el llamador reintenta un render real y registra el resultado
    /// con `revive`/`record`. Sin esto, un fallo transitorio congelaría el
    /// lienzo hasta el resize aunque la GPU ya estuviera sana.
    fn probe_due(&mut self) -> bool {
        if !self.dead {
            return false;
        }
        self.dead_frames = self.dead_frames.saturating_add(1);
        if self.dead_frames >= GPU_PROBE_EVERY {
            self.dead_frames = 0;
            true
        } else {
            false
        }
    }

    /// La sonda salió bien: la GPU responde, se reanuda el render normal.
    fn revive(&mut self) {
        self.dead = false;
        self.streak = 0;
        self.dead_frames = 0;
    }
}

/// Cachea por viewport un presupuesto medido del sistema, con caducidad
/// (`deck::ram_probe_due`). Lo usan el presupuesto GPU de efectos y el de la
/// baraja, que corren en CADA frame: medir la RAM en cada frame es una
/// syscall por frame sin ganancia (la presión no cambia a 60 Hz). El gate de
/// guardado/exportación NO pasa por aquí: mide en vivo, en el momento del
/// bake (`app::persistence`).
pub(crate) fn cached_render_budget<T>(
    ctx: &egui::Context,
    key: &'static str,
    measure: fn() -> T,
) -> T
where
    T: Copy + Send + Sync + 'static,
{
    let id = egui::Id::new((key, ctx.viewport_id()));
    let now = std::time::Instant::now();
    if let Some((at, value)) = ctx.data(|d| d.get_temp::<(std::time::Instant, T)>(id)) {
        if !crate::deck::ram_probe_due(Some(at), now, crate::deck::RAM_PROBE_INTERVAL) {
            return value;
        }
    }
    let value = measure();
    ctx.data_mut(|d| d.insert_temp(id, (now, value)));
    value
}

pub struct CanvasSurface {
    /// Vista de la textura de destino, creada UNA vez por tamaño. La vista
    /// mantiene viva la textura (wgpu la referencia desde dentro), así que no
    /// hace falta guardar el `Texture` aparte. `render` creaba una vista
    /// nueva en cada frame: un objeto wgpu por frame sin ninguna razón, ya
    /// que la textura solo cambia al redimensionar (ver `ensure`).
    view: wgpu::TextureView,
    egui_id: egui::TextureId,
    size: [u32; 2],
    health: GpuHealth,
    /// Buffer de escena reutilizado entre frames. `vello::Scene::reset()`
    /// vacía el contenido sin dealocar la capacidad ya reservada — evita
    /// una allocación por frame (la `Scene` puede crecer a varios KB con
    /// muchos caminos de forma).
    scene: vello::Scene,
}

impl CanvasSurface {
    /// Garantiza que el slot contiene una textura del tamaño pedido,
    /// (re)registrándola en egui si hace falta.
    pub fn ensure<'a>(
        slot: &'a mut Option<CanvasSurface>,
        rs: &RenderState,
        width: u32,
        height: u32,
    ) -> &'a mut CanvasSurface {
        let width = width.max(1);
        let height = height.max(1);
        let stale = match slot {
            Some(s) => s.size != [width, height],
            None => true,
        };
        if stale {
            let texture = CanvasRenderer::create_target_texture(&rs.device, width, height);
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut egui_renderer = rs.renderer.write();
            let egui_id = match slot.take() {
                Some(old) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        &rs.device,
                        &view,
                        wgpu::FilterMode::Nearest,
                        old.egui_id,
                    );
                    old.egui_id
                }
                None => egui_renderer.register_native_texture(
                    &rs.device,
                    &view,
                    wgpu::FilterMode::Nearest,
                ),
            };
            drop(egui_renderer);
            *slot = Some(CanvasSurface {
                view,
                egui_id,
                size: [width, height],
                health: GpuHealth {
                    streak: 0,
                    dead: false,
                    dead_frames: 0,
                },
                scene: vello::Scene::new(),
            });
        }
        slot.as_mut()
            .unwrap_or_else(|| unreachable!("recién asegurado"))
    }

    /// Devuelve la `Scene` reutilizable de este frame. El llamador la
    /// llena con `append_document` y luego llama `render`.
    pub fn scene_mut(&mut self) -> &mut vello::Scene {
        self.scene.reset();
        &mut self.scene
    }

    /// Devuelve la `Scene` ya llena para renderizar (préstamo inmutable,
    /// compatible con `render` que toma `&self`).
    pub fn scene_ref(&self) -> &vello::Scene {
        &self.scene
    }

    pub fn render(
        &self,
        rs: &RenderState,
        renderer: &mut CanvasRenderer,
        scene: &vello::Scene,
    ) -> Result<(), RenderError> {
        renderer.render_to_texture(
            &rs.device,
            &rs.queue,
            scene,
            &self.view,
            self.size[0],
            self.size[1],
        )
    }

    pub fn egui_id(&self) -> egui::TextureId {
        self.egui_id
    }

    /// Registra el resultado del render de este frame. Con la GPU muerta ya
    /// no se le envía trabajo (`gpu_dead`): el llamador salta `render` salvo
    /// en la sonda periódica (`gpu_probe_due` + `record_probe`).
    pub fn record_render(&mut self, ok: bool) {
        let was_dead = self.health.dead;
        self.health.record(ok);
        if self.health.dead && !was_dead {
            tracing::error!(
                racha = self.health.streak,
                "vello falla de forma sostenida: render detenido, la UI sigue viva"
            );
        }
    }

    /// ¿El render está cortado? El lienzo muestra la última textura buena.
    pub fn gpu_dead(&self) -> bool {
        self.health.dead
    }

    /// ¿Toca un render de sonda tras el corte? (ver `GpuHealth`.)
    pub fn gpu_probe_due(&mut self) -> bool {
        self.health.probe_due()
    }

    /// Registra la sonda: un éxito reanuda el render normal; un fallo la
    /// deja cortada sin ruido (ya se avisó al disparar el corte).
    pub fn record_probe(&mut self, ok: bool) {
        if ok && self.health.dead {
            self.health.revive();
            tracing::info!("la GPU responde de nuevo: render reanudado");
        }
    }

    /// Fallos seguidos actuales (0 en operación normal): lo enseña la barra
    /// de estado antes de que el corte dispare.
    pub fn gpu_fail_streak(&self) -> u32 {
        self.health.streak
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    use super::{cached_render_budget, GpuHealth, GPU_FAIL_TRIP, GPU_PROBE_EVERY};

    /// El presupuesto de render se mide UNA vez por intervalo: el segundo
    /// frame dentro del mismo segundo reutiliza el valor (cero syscalls).
    #[test]
    fn render_budget_is_measured_once_per_interval() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        fn measure() -> usize {
            CALLS.fetch_add(1, Ordering::Relaxed);
            42
        }
        let ctx = egui::Context::default();
        assert_eq!(cached_render_budget(&ctx, "test_budget", measure), 42);
        assert_eq!(cached_render_budget(&ctx, "test_budget", measure), 42);
        assert_eq!(
            CALLS.load(Ordering::Relaxed),
            1,
            "midió dos veces en el intervalo"
        );
        // Clave distinta (otro presupuesto): mide por su cuenta.
        assert_eq!(cached_render_budget(&ctx, "test_budget_2", measure), 42);
        assert_eq!(CALLS.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn isolated_failures_do_not_trip_the_breaker() {
        let mut health = GpuHealth {
            streak: 0,
            dead: false,
            dead_frames: 0,
        };
        for _ in 0..GPU_FAIL_TRIP {
            health.record(false);
            health.record(true);
        }
        assert!(!health.dead);
        assert_eq!(health.streak, 0);
    }

    #[test]
    fn sustained_failures_trip_and_latch() {
        let mut health = GpuHealth {
            streak: 0,
            dead: false,
            dead_frames: 0,
        };
        for _ in 0..GPU_FAIL_TRIP {
            health.record(false);
        }
        assert!(health.dead);
        // Enganchado en el camino normal: ya no se renderiza, así que un
        // éxito no llega por `record` (solo la sonda de abajo lo rearma).
        health.record(true);
        assert!(health.dead);
    }

    #[test]
    fn probe_retries_after_the_cut_and_revives_on_success() {
        let mut health = GpuHealth {
            streak: 0,
            dead: false,
            dead_frames: 0,
        };
        for _ in 0..GPU_FAIL_TRIP {
            health.record(false);
        }
        assert!(health.dead);
        // Sin corte no hay sonda; con corte, una cada `GPU_PROBE_EVERY`.
        for _ in 0..GPU_PROBE_EVERY - 1 {
            assert!(!health.probe_due());
        }
        assert!(health.probe_due());
        assert!(!health.probe_due());
        // La sonda con éxito reanuda; un fallo aislado ya no corta.
        health.revive();
        assert!(!health.dead);
        health.record(false);
        assert!(!health.dead);
        assert_eq!(health.streak, 1);
    }
}
