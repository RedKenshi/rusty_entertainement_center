//! libmpv video output via an OpenGL RGBA texture displayed as a Slint `Image`.
//!
//! Based on the approach from https://github.com/maurges/slint-mpv-widget

use std::cell::RefCell;
use std::ffi::{c_void, CStr, CString};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::mpsc;

use glow::HasContext;
use i_slint_core::graphics::IntSize;
use libmpv2::events::Event;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use libmpv2::Mpv;
use slint::{BorrowedOpenGLTextureBuilder, ComponentHandle};

use crate::debug;
use super::state::{PlaybackState, PlaybackStatus};
use super::tracks::{
    audio_track_count, current_audio_label, current_subtitle_label, refresh_playback_tracks,
    subtitle_track_count,
};
use super::{PlayerCommand, PlayerEvent};

struct GlCtx(&'static dyn Fn(&CStr) -> *const c_void);

fn resolve_gl_proc(ctx: &GlCtx, name: &str) -> *mut c_void {
    let name = CString::new(name).expect("invalid GL symbol name");
    ctx.0(name.as_c_str()) as *mut c_void
}

struct VideoTexture {
    texture: glow::Texture,
    fbo: glow::Framebuffer,
    width: u32,
    height: u32,
    gl: Rc<glow::Context>,
}

impl VideoTexture {
    unsafe fn new(gl: &Rc<glow::Context>, width: u32, height: u32) -> Self {
        let fbo = gl.create_framebuffer().expect("cannot create framebuffer");
        let texture = gl.create_texture().expect("cannot create texture");

        let prev_texture = NonZeroU32::new(gl.get_parameter_i32(glow::TEXTURE_BINDING_2D) as u32)
            .map(glow::NativeTexture);
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );

        let prev_fbo = NonZeroU32::new(gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) as u32)
            .map(glow::NativeFramebuffer);
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        debug_assert_eq!(
            gl.check_framebuffer_status(glow::FRAMEBUFFER),
            glow::FRAMEBUFFER_COMPLETE
        );

        gl.bind_texture(glow::TEXTURE_2D, prev_texture);
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, prev_fbo);

        Self {
            texture,
            fbo,
            width,
            height,
            gl: Rc::clone(gl),
        }
    }

    unsafe fn with_active_fbo<R>(&self, draw: impl FnOnce() -> R) -> R {
        let prev_fbo = NonZeroU32::new(self.gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) as u32)
            .map(glow::NativeFramebuffer);
        self.gl
            .bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.fbo));
        let result = draw();
        self.gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, prev_fbo);
        result
    }
}

impl Drop for VideoTexture {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_texture(self.texture);
        }
    }
}

struct MpvVideoLayer {
    gl: Rc<glow::Context>,
    render_ctx: RenderContext<'static>,
    mpv: Box<Mpv>,
    texture: Option<VideoTexture>,
    playback: PlaybackState,
    needs_redraw: bool,
}

impl MpvVideoLayer {
    fn new(
        gl: glow::Context,
        get_proc_address: &'static dyn Fn(&CStr) -> *const c_void,
    ) -> Self {
        let proc_ctx = GlCtx(get_proc_address);
        let gl = Rc::new(gl);

        let mpv = Box::new(
            Mpv::with_initializer(|init| {
                init.set_option("vo", "libmpv")?;
                init.set_option("hwdec", "auto")?;
                init.set_option("keep-open", "no")?;
                init.set_option("video-timing-offset", "0")?;
                init.set_option("sub-visibility", "yes")?;
                init.set_option("sub-auto", "fuzzy")?;
                Ok(())
            })
            .expect("failed to create mpv"),
        );

        let render_ctx = mpv
            .create_render_context(vec![
                RenderParam::ApiType(RenderParamApiType::OpenGl),
                RenderParam::InitParams(OpenGLInitParams {
                    get_proc_address: resolve_gl_proc,
                    ctx: proc_ctx,
                }),
            ])
            .expect("failed to create mpv render context");
        let render_ctx = unsafe {
            std::mem::transmute::<RenderContext<'_>, RenderContext<'static>>(render_ctx)
        };

        let mut render_ctx = render_ctx;
        render_ctx.set_update_callback(|| {
            NEEDS_REDRAW.with(|flag| flag.set(true));
        });

        Self {
            gl,
            render_ctx,
            mpv,
            texture: None,
            playback: PlaybackState::default(),
            needs_redraw: true,
        }
    }

    fn tick(
        &mut self,
        command_rx: &mpsc::Receiver<PlayerCommand>,
        event_tx: &mpsc::Sender<PlayerEvent>,
        player_active: bool,
        width: u32,
        height: u32,
        window: &crate::ui::MainWindow,
    ) -> bool {
        while let Ok(command) = command_rx.try_recv() {
            self.apply_command(command, event_tx);
        }

        self.poll_events(event_tx);

        if player_active && self.playback.path.is_some() && width > 0 && height > 0 {
            self.sync_state_from_mpv();
            let _ = event_tx.send(PlayerEvent::State(self.playback.clone()));

            if let Some(image) = self.render_to_image(width, height) {
                window.set_playback_video(image);
            }

            return true;
        }

        false
    }

    fn render_to_image(&mut self, width: u32, height: u32) -> Option<slint::Image> {
        let recreated = match &self.texture {
            Some(texture) if texture.width == width && texture.height == height => false,
            _ => true,
        };

        if recreated {
            self.texture = Some(unsafe { VideoTexture::new(&self.gl, width, height) });
        }

        let texture = self.texture.as_ref()?;
        let fbo_id = texture.fbo.0.get();

        unsafe {
            texture.with_active_fbo(|| {
                let mut saved_viewport = [0i32; 4];
                self.gl
                    .get_parameter_i32_slice(glow::VIEWPORT, &mut saved_viewport);
                self.gl.viewport(0, 0, width as i32, height as i32);

                if let Err(err) = self.render_ctx.render::<GlCtx>(
                    fbo_id as i32,
                    width as i32,
                    height as i32,
                    false,
                ) {
                    debug::player(format!("mpv render failed: {err}"));
                } else {
                    self.render_ctx.report_swap();
                }

                self.gl.viewport(
                    saved_viewport[0],
                    saved_viewport[1],
                    saved_viewport[2],
                    saved_viewport[3],
                );
            });
        }

        self.needs_redraw = false;
        NEEDS_REDRAW.with(|flag| flag.set(false));

        let texture_id = texture.texture.0;
        Some(unsafe {
            BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                texture_id,
                IntSize::new(width, height),
            )
            .build()
        })
    }

    fn apply_command(&mut self, command: PlayerCommand, event_tx: &mpsc::Sender<PlayerEvent>) -> bool {
        match command {
            PlayerCommand::Open {
                path,
                title,
                resume_ms,
                duration_ms,
            } => {
                let path_str = path.to_string_lossy();
                let _ = self.mpv.set_property("start", 0.0_f64);
                if let Some(resume_ms) = resume_ms {
                    let _ = self
                        .mpv
                        .set_property("start", resume_ms as f64 / 1000.0);
                }

                if let Err(err) = self.mpv.command("loadfile", &[path_str.as_ref(), "replace"]) {
                    debug::player(format!("mpv loadfile failed: {err}"));
                }
                let _ = self.mpv.set_property("pause", false);

                debug::player(format!("mpv open {}", path.display()));
                self.playback = PlaybackState {
                    path: Some(path),
                    title,
                    status: PlaybackStatus::Playing,
                    position_ms: resume_ms.unwrap_or(0),
                    duration_ms,
                    ..PlaybackState::default()
                };
                self.needs_redraw = true;
                false
            }
            PlayerCommand::Play => {
                let _ = self.mpv.set_property("pause", false);
                if self.playback.path.is_some() {
                    self.playback.status = PlaybackStatus::Playing;
                }
                false
            }
            PlayerCommand::Pause => {
                let _ = self.mpv.set_property("pause", true);
                if self.playback.path.is_some() {
                    self.playback.status = PlaybackStatus::Paused;
                }
                false
            }
            PlayerCommand::TogglePause => {
                let paused: bool = self.mpv.get_property("pause").unwrap_or(false);
                let _ = self.mpv.set_property("pause", !paused);
                if self.playback.path.is_some() {
                    self.playback.status = if paused {
                        PlaybackStatus::Playing
                    } else {
                        PlaybackStatus::Paused
                    };
                }
                false
            }
            PlayerCommand::Stop => {
                let _ = self.mpv.command("stop", &[]);
                self.reset_playback();
                true
            }
            PlayerCommand::SeekTo(position_ms) => {
                let secs = position_ms as f64 / 1000.0;
                let _ = self.mpv.command("seek", &[&secs.to_string(), "absolute"]);
                self.playback.apply_seek_to(position_ms);
                self.needs_redraw = true;
                false
            }
            PlayerCommand::SeekDelta(delta_ms) => {
                let secs = delta_ms as f64 / 1000.0;
                let _ = self.mpv.command("seek", &[&secs.to_string(), "relative"]);
                self.playback.apply_seek_delta(delta_ms);
                self.needs_redraw = true;
                false
            }
            PlayerCommand::CycleAudioTrack => {
                if audio_track_count(&self.mpv) <= 1 {
                    return false;
                }
                let _ = self.mpv.command("cycle", &["audio"]);
                self.refresh_tracks();
                let _ = event_tx.send(PlayerEvent::TrackToast(current_audio_label(&self.mpv)));
                false
            }
            PlayerCommand::CycleSubtitleTrack => {
                if subtitle_track_count(&self.mpv) == 0 {
                    return false;
                }
                let _ = self.mpv.command("cycle", &["sub"]);
                self.refresh_tracks();
                let _ = event_tx.send(PlayerEvent::TrackToast(current_subtitle_label(&self.mpv)));
                false
            }
            PlayerCommand::Shutdown => false,
        }
    }

    fn poll_events(&mut self, event_tx: &mpsc::Sender<PlayerEvent>) {
        loop {
            match self.mpv.wait_event(0.0) {
                Some(Ok(Event::EndFile(reason))) => {
                    debug::player(format!("mpv end file: {reason:?}"));
                    if self.playback.path.is_some() {
                        self.reset_playback();
                        let _ = event_tx.send(PlayerEvent::Stopped);
                    }
                    break;
                }
                Some(Ok(Event::LogMessage { level, text, .. })) => {
                    debug::player(format!("mpv [{level}]: {text}"));
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => {
                    debug::player(format!("mpv event error: {err}"));
                    break;
                }
                None => break,
            }
        }
    }

    fn sync_state_from_mpv(&mut self) {
        if self.playback.path.is_none() {
            return;
        }

        let paused: bool = self.mpv.get_property("pause").unwrap_or(true);
        self.playback.status = if paused {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        };

        let position_secs: f64 = self.mpv.get_property("time-pos").unwrap_or(0.0);
        self.playback.position_ms = (position_secs * 1000.0).max(0.0) as u64;

        if let Ok(duration_secs) = self.mpv.get_property::<f64>("duration") {
            if duration_secs.is_finite() && duration_secs > 0.0 {
                self.playback.duration_ms = Some((duration_secs * 1000.0) as u64);
            }
        }

        self.refresh_tracks();
    }

    fn refresh_tracks(&mut self) {
        refresh_playback_tracks(
            &self.mpv,
            &mut self.playback.audio_tracks,
            &mut self.playback.subtitle_tracks,
        );
        self.playback.selected_audio = super::tracks::current_aid(&self.mpv)
            .map(|id| id as u32)
            .unwrap_or(0);
        self.playback.selected_subtitle = super::tracks::current_sid(&self.mpv).map(|id| id as u32);
    }

    fn reset_playback(&mut self) {
        self.playback = PlaybackState::default();
        self.texture = None;
        self.needs_redraw = true;
    }
}

thread_local! {
    static NEEDS_REDRAW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn wire_mpv_video_layer(
    window: &crate::ui::MainWindow,
    command_rx: mpsc::Receiver<PlayerCommand>,
    event_tx: mpsc::Sender<PlayerEvent>,
    player_active: Rc<RefCell<bool>>,
) {
    let window_weak = window.as_weak();
    let mut layer: Option<MpvVideoLayer> = None;

    if let Err(error) = window.window().set_rendering_notifier(move |state, graphics_api| {
        match state {
            slint::RenderingState::RenderingSetup => {
                match graphics_api {
                    slint::GraphicsAPI::NativeOpenGL { get_proc_address } => {
                        let proc_static: &'static dyn Fn(&CStr) -> *const c_void = unsafe {
                            std::mem::transmute(*get_proc_address)
                        };
                        let gl = unsafe {
                            glow::Context::from_loader_function_cstr(|name| proc_static(name))
                        };
                        layer = Some(MpvVideoLayer::new(gl, proc_static));
                        debug::player("mpv OpenGL video layer initialized");
                    }
                    _ => {
                        debug::player(
                            "video layer skipped: Slint backend is not NativeOpenGL",
                        );
                    }
                }
            }
            slint::RenderingState::BeforeRendering => {
                let Some(app) = window_weak.upgrade() else {
                    return;
                };
                let Some(layer) = layer.as_mut() else {
                    return;
                };

                let size = app.window().size();
                let width = size.width;
                let height = size.height;
                let active = *player_active.borrow();

                let keep_animating = layer.tick(
                    &command_rx,
                    &event_tx,
                    active,
                    width,
                    height,
                    &app,
                );
                if keep_animating {
                    app.window().request_redraw();
                }
            }
            slint::RenderingState::RenderingTeardown => {
                drop(layer.take());
            }
            _ => {}
        }
    }) {
        match error {
            slint::SetRenderingNotifierError::Unsupported => {
                eprintln!(
                    "Video playback requires an OpenGL Slint backend. \
                     Desktop: SLINT_BACKEND=opengl  |  Pi kiosk: --features kiosk"
                );
            }
            _ => unreachable!("unexpected rendering notifier error"),
        }
    }
}
