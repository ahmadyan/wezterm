//! Embedding glue that wires our terminal handle into wezterm-gui's actual
//! rendering pipeline.
//!
//! Verbatim wezterm-gui modules used:
//!   - `glyphcache::GlyphCache` — glyph rasterization with font fallback,
//!     color emoji, .notdef substitution, ligatures, custom block glyphs.
//!   - `renderstate::RenderState` — render pipeline, triple-buffered vertex
//!     buffers, atlas management, quad allocator.
//!   - `termwindow::webgpu::WebGpuState` — wgpu surface, adapter, device,
//!     queue, render pipeline, sampler / bind group setup.
//!   - `utilsprites::RenderMetrics` — cell metrics derived from font config.
//!   - `quad::{QuadTrait, TripleLayerQuadAllocatorTrait}` — quad emission.
//!
//! What this file adds:
//!   - A small render loop (~120 lines) that walks the cells of a
//!     `wezterm_term::Terminal`, calls `font.shape()` + `glyph_cache.cached_glyph()`
//!     for each cell, and emits quads via the wezterm-gui quad allocator.
//!   - The frame submission code is adapted from `TermWindow::call_draw_webgpu`
//!     in wezterm-gui/src/termwindow/render/draw.rs.

use anyhow::Result;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::Rc;

use config::ConfigHandle;
use wezterm_font::FontConfiguration;
use wezterm_gui::glyphcache::GlyphCache;
use wezterm_gui::quad::{QuadTrait, TripleLayerQuadAllocatorTrait};
use wezterm_gui::renderstate::{RenderContext, RenderState};
use wezterm_gui::shapecache::{ShapeCacheKey, ShapedInfo};
use wezterm_gui::termwindow::webgpu::{RawHandlePair, ShaderUniform, WebGpuState, WebGpuTexture};
use wezterm_gui::utilsprites::RenderMetrics;

use ::window::color::LinearRgba;
use ::window::Dimensions;

use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};

use termwiz::cellcluster::CellCluster;
use wezterm_term::color::ColorAttribute;
use wezterm_term::Terminal;
use wezterm_color_types::SrgbaTuple;
use wezterm_font::shaper::PresentationWidth;

// ---------------------------------------------------------------------------
// GpuRenderer — owns wezterm-gui's RenderState + WebGpuState
// ---------------------------------------------------------------------------

pub struct GpuRenderer {
    webgpu: Rc<WebGpuState>,
    render_state: RenderState,
    font_config: Rc<FontConfiguration>,
    metrics: RenderMetrics,
    width: u32,
    height: u32,
    _scale_factor: f64,

    // Dirty-tracking: skip rendering if the terminal state hasn't changed
    // since the last frame. The display link fires at ~60fps but the terminal
    // is idle most of the time, so this turns idle frames into a no-op.
    last_rendered_seqno: u64,
    // Also re-render when these change (theme color or window resize)
    last_bg_color: [f32; 4],
    last_cursor_color: [f32; 4],
    last_width: u32,
    last_height: u32,
    last_viewport_offset: isize,
    last_palette: Option<wezterm_term::color::ColorPalette>,

    // Cluster-level shape cache. This mirrors upstream more closely than the
    // old per-cell cache and preserves ligatures/combining marks/wide glyphs.
    // As with upstream, the cache key is style + text.
    shape_cache: HashMap<ShapeCacheKey, Rc<Vec<ShapedInfo>>>,
}

impl GpuRenderer {
    pub fn new(
        nsview: *mut libc::c_void,
        width: u32,
        height: u32,
        scale_factor: f64,
        config: &ConfigHandle,
        font_config: Rc<FontConfiguration>,
    ) -> Result<Self> {
        // Compute cell metrics from font configuration (verbatim wezterm-gui)
        let metrics = RenderMetrics::new(&font_config)?;

        // Build a RawHandlePair from the NSView pointer.
        // RawHandlePair::from_raw is a small additive helper we added to
        // wezterm-gui/src/termwindow/webgpu.rs for embedders.
        let nsview_nn = NonNull::new(nsview as *mut _).expect("nsview must not be null");
        let win_handle = RawWindowHandle::AppKit(AppKitWindowHandle::new(nsview_nn));
        let display_handle = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());
        let handle = RawHandlePair::from_raw(win_handle, display_handle);

        let dimensions = Dimensions {
            pixel_width: width as usize,
            pixel_height: height as usize,
            dpi: font_config.get_dpi(),
        };

        // Create WebGpuState verbatim from wezterm-gui
        let webgpu = pollster::block_on(WebGpuState::new_impl(handle, dimensions, config))?;
        let webgpu = Rc::new(webgpu);

        // Create RenderState verbatim from wezterm-gui (sets up GlyphCache,
        // UtilSprites, vertex buffers, etc.)
        let context = RenderContext::WebGpu(Rc::clone(&webgpu));
        let render_state = RenderState::new(context, &font_config, &metrics, 4096)?;

        Ok(Self {
            webgpu,
            render_state,
            font_config,
            metrics,
            width,
            height,
            _scale_factor: scale_factor,
            last_rendered_seqno: u64::MAX, // sentinel: force first render
            last_bg_color: [f32::NAN; 4],
            last_cursor_color: [f32::NAN; 4],
            last_width: 0,
            last_height: 0,
            last_viewport_offset: isize::MAX, // sentinel: force first render
            last_palette: None,
            shape_cache: HashMap::new(),
        })
    }

    pub fn cell_size(&self) -> (f64, f64) {
        (
            self.metrics.cell_size.width as f64,
            self.metrics.cell_size.height as f64,
        )
    }

    pub fn resize(&mut self, width: u32, height: u32, scale_factor: f64) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self._scale_factor = scale_factor;
        let dimensions = Dimensions {
            pixel_width: width as usize,
            pixel_height: height as usize,
            dpi: self.font_config.get_dpi(),
        };
        self.webgpu.resize(dimensions);
        // Force the next render() to actually run (size mismatch with last_*).
        // The seqno-based dirty check would otherwise skip it.
        self.last_width = 0;
        self.last_height = 0;
        // DPI may have changed; the cached glyph advances are DPI-dependent.
        self.shape_cache.clear();
    }

    pub fn set_font_config(&mut self, font_config: Rc<FontConfiguration>) -> Result<()> {
        self.font_config = font_config;
        self.metrics = RenderMetrics::new(&self.font_config)?;
        let context = RenderContext::WebGpu(Rc::clone(&self.webgpu));
        self.render_state = RenderState::new(context, &self.font_config, &self.metrics, 4096)?;

        let dimensions = Dimensions {
            pixel_width: self.width as usize,
            pixel_height: self.height as usize,
            dpi: self.font_config.get_dpi(),
        };
        self.webgpu.resize(dimensions);

        self.last_rendered_seqno = u64::MAX;
        self.last_width = 0;
        self.last_height = 0;
        self.shape_cache.clear();
        Ok(())
    }

    /// Render the terminal content. Walks line clusters and emits quads via
    /// wezterm-gui's quad allocator. This preserves ligatures, combining marks,
    /// double-width glyphs and font-rule based style selection.
    ///
    /// `viewport_offset` controls scrollback viewing:
    /// - 0 means showing the live terminal output
    /// - positive values scroll back into history
    pub fn render(
        &mut self,
        terminal: &mut Terminal,
        bg_color: [f32; 4],
        cursor_color: [f32; 4],
        viewport_offset: isize,
    ) -> Result<()> {
        // Skip render if nothing has changed since last frame.
        // This is the most important optimization: the display link fires at
        // ~60fps but the terminal is idle most of the time. Without this,
        // we'd re-shape every cell with harfbuzz on every tick.
        let current_seqno = terminal.current_seqno() as u64;
        let palette = terminal.palette();
        let seqno_changed = current_seqno != self.last_rendered_seqno;
        let size_changed = self.width != self.last_width || self.height != self.last_height;
        let colors_changed = bg_color != self.last_bg_color
            || cursor_color != self.last_cursor_color;
        let viewport_changed = viewport_offset != self.last_viewport_offset;
        let palette_changed = self.last_palette.as_ref() != Some(&palette);
        if !seqno_changed
            && !size_changed
            && !colors_changed
            && !viewport_changed
            && !palette_changed
        {
            return Ok(());
        }

        for pass in 0.. {
            self.paint_pass(terminal, &palette, bg_color, cursor_color, viewport_offset)?;
            if !self.render_state.allocated_more_quads()? {
                break;
            }
            if pass >= 3 {
                anyhow::bail!("quad buffer allocation did not settle after {} passes", pass + 1);
            }
        }

        self.draw_frame()?;

        self.last_rendered_seqno = current_seqno;
        self.last_bg_color = bg_color;
        self.last_cursor_color = cursor_color;
        self.last_width = self.width;
        self.last_height = self.height;
        self.last_viewport_offset = viewport_offset;
        self.last_palette = Some(palette);
        Ok(())
    }

    fn paint_pass(
        &mut self,
        terminal: &mut Terminal,
        palette: &wezterm_term::color::ColorPalette,
        bg_color: [f32; 4],
        cursor_color: [f32; 4],
        viewport_offset: isize,
    ) -> Result<()> {
        let cell_w = self.metrics.cell_size.width as f32;
        let cell_h = self.metrics.cell_size.height as f32;
        let descender = self.metrics.descender.get() as f32;

        let cursor = terminal.cursor_pos();
        let reverse_video = terminal.get_reverse_video();
        let screen = terminal.screen_mut();
        let visible_rows = screen.physical_rows;
        let cols = screen.physical_cols;
        let total_rows = screen.scrollback_rows();

        // Compute the background sprite texture coords before borrowing the
        // layer below, so the immutable read of render_state.util_sprites does
        // not overlap the mutable layer borrow.
        let filled_box = self.render_state.util_sprites.filled_box.texture_coords();

        let layer = self.render_state.layer_for_zindex(0)?;
        layer.clear_quad_allocation();

        let mut quads = layer.quad_allocator();

        let cfg = self.font_config.config();

        let half_w = self.width as f32 / 2.0;
        let half_h = self.height as f32 / 2.0;
        let origin_x = -half_w;
        let origin_y = -half_h;

        {
            let bg_linear =
                SrgbaTuple(bg_color[0], bg_color[1], bg_color[2], bg_color[3]).to_linear();
            let mut q = quads.allocate(0)?;
            q.set_position(
                origin_x,
                origin_y,
                origin_x + self.width as f32,
                origin_y + self.height as f32,
            );
            q.set_texture(filled_box);
            q.set_fg_color(bg_linear);
            q.set_hsv(None);
            q.set_is_background();
        }

        let mut glyph_cache = self.render_state.glyph_cache.borrow_mut();
        let mut shape_cache = std::mem::take(&mut self.shape_cache);

        // Select which physical rows to render based on viewport offset.
        // viewport_offset == 0 shows the live terminal (bottom of scrollback);
        // positive values scroll back into history.
        let first_phys_row = if viewport_offset == 0 {
            total_rows.saturating_sub(visible_rows)
        } else {
            total_rows
                .saturating_sub(visible_rows)
                .saturating_sub(viewport_offset as usize)
        };

        for row_idx in 0..visible_rows {
            let phys_row = first_phys_row + row_idx;
            if phys_row >= total_rows {
                break;
            }
            let line = screen.line_mut(phys_row);
            let row_top = origin_y + row_idx as f32 * cell_h;
            let row_bottom = row_top + cell_h;
            let (bidi_enabled, bidi_direction) = line.bidi_info();
            let bidi_hint = if bidi_enabled { Some(bidi_direction) } else { None };

            for cluster in line.cluster(bidi_hint) {
                if cluster.first_cell_idx >= cols {
                    break;
                }

                let attrs = &cluster.attrs;
                let cluster_width = cluster.width.min(cols.saturating_sub(cluster.first_cell_idx));
                if cluster_width == 0 {
                    continue;
                }

                let cluster_left = origin_x + cluster.first_cell_idx as f32 * cell_w;
                let cluster_right = cluster_left + cluster_width as f32 * cell_w;

                let (fg_linear, bg_linear, bg_is_default) =
                    Self::resolve_cluster_colors(&cfg, palette, attrs, reverse_video);

                if !bg_is_default {
                    let mut q = quads.allocate(0)?;
                    q.set_position(cluster_left, row_top, cluster_right, row_bottom);
                    q.set_texture(filled_box);
                    q.set_fg_color(bg_linear);
                    q.set_hsv(None);
                    q.set_is_background();
                }

                if attrs.invisible()
                    || cluster.text.is_empty()
                    || cluster.text.chars().all(|ch| ch == ' ')
                {
                    continue;
                }

                let style = self.font_config.match_style(&cfg, attrs);
                let font = match self.font_config.resolve_font(style) {
                    Ok(font) => font,
                    Err(_) => continue,
                };

                let shaped = match self.cached_cluster_shape(
                    &cluster,
                    style,
                    &font,
                    &mut glyph_cache,
                    &mut shape_cache,
                ) {
                    Ok(shaped) => shaped,
                    Err(_) => continue,
                };
                let mut cell_x = cluster_left;
                for shaped_info in shaped.iter() {
                    let glyph = &shaped_info.glyph;
                    if let Some(sprite) = &glyph.texture {
                        let glyph_w = sprite.coords.size.width as f32 * glyph.scale as f32;
                        let glyph_h = sprite.coords.size.height as f32 * glyph.scale as f32;
                        let baseline_y = row_bottom + descender;
                        let glyph_y = baseline_y
                            - (glyph.y_offset.get() as f32 + glyph.bearing_y.get() as f32);
                        let glyph_x = cell_x
                            + (glyph.x_offset.get() as f32 + glyph.bearing_x.get() as f32);

                        let mut q = quads.allocate(1)?;
                        q.set_position(glyph_x, glyph_y, glyph_x + glyph_w, glyph_y + glyph_h);
                        q.set_texture(sprite.texture_coords());
                        q.set_fg_color(fg_linear);
                        q.set_hsv(None);
                        q.set_has_color(glyph.has_color);
                    }

                    cell_x += shaped_info.pos.num_cells as f32 * cell_w;
                }
            }
        }

        // Cursor — only when viewing the live terminal (viewport_offset == 0).
        if viewport_offset == 0
            && matches!(cursor.visibility, wezterm_surface::CursorVisibility::Visible)
            && (cursor.y as usize) < visible_rows
        {
            let cx = origin_x + cursor.x as f32 * cell_w;
            let cy = origin_y + cursor.y as f32 * cell_h;
            let cursor_linear = SrgbaTuple(
                cursor_color[0],
                cursor_color[1],
                cursor_color[2],
                cursor_color[3],
            )
            .to_linear();
            let mut q = quads.allocate(2)?;
            q.set_position(cx, cy, cx + cell_w, cy + cell_h);
            q.set_texture(filled_box);
            q.set_fg_color(cursor_linear);
            q.set_hsv(None);
            q.set_is_background();
        }

        drop(quads);
        drop(glyph_cache);
        self.shape_cache = shape_cache;

        Ok(())
    }

    fn cached_cluster_shape(
        &self,
        cluster: &CellCluster,
        style: &config::TextStyle,
        font: &Rc<wezterm_font::LoadedFont>,
        glyph_cache: &mut GlyphCache,
        shape_cache: &mut HashMap<ShapeCacheKey, Rc<Vec<ShapedInfo>>>,
    ) -> Result<Rc<Vec<ShapedInfo>>> {
        let key = ShapeCacheKey {
            style: style.clone(),
            text: cluster.text.clone(),
        };

        if let Some(cached) = shape_cache.get(&key) {
            return Ok(Rc::clone(cached));
        }

        let presentation_width = PresentationWidth::with_cluster(cluster);
        let infos = font.shape(
            &cluster.text,
            || {},
            |_| {},
            Some(cluster.presentation),
            cluster.direction,
            None,
            Some(&presentation_width),
        )?;

        let mut glyphs = Vec::with_capacity(infos.len());
        let mut iter = infos.iter().peekable();
        while let Some(info) = iter.next() {
            let followed_by_space = iter.peek().map(|next| next.is_space).unwrap_or(false);
            glyphs.push(glyph_cache.cached_glyph(
                info,
                style,
                followed_by_space,
                font,
                &self.metrics,
                info.num_cells,
            )?);
        }

        let shaped = Rc::new(ShapedInfo::process(&infos, &glyphs));
        let all_resolved = !shaped.is_empty() && shaped.iter().all(|info| info.pos.glyph_idx != 0);

        if all_resolved {
            shape_cache.insert(key, Rc::clone(&shaped));
        }

        Ok(shaped)
    }

    fn resolve_cluster_colors(
        cfg: &ConfigHandle,
        palette: &wezterm_term::color::ColorPalette,
        attrs: &wezterm_term::CellAttributes,
        reverse_video: bool,
    ) -> (LinearRgba, LinearRgba, bool) {
        let fg_attr = attrs.foreground();
        let mut fg = match fg_attr {
            ColorAttribute::Default => palette.resolve_fg(fg_attr),
            ColorAttribute::PaletteIndex(idx)
                if idx < 8 && cfg.bold_brightens_ansi_colors != config::BoldBrightening::No =>
            {
                let idx = if attrs.intensity() == wezterm_term::Intensity::Bold {
                    idx + 8
                } else {
                    idx
                };
                palette.resolve_fg(ColorAttribute::PaletteIndex(idx))
            }
            _ => palette.resolve_fg(fg_attr),
        };
        let mut bg = palette.resolve_bg(attrs.background());
        let mut bg_is_default = attrs.background() == ColorAttribute::Default;

        // Cell-level reverse XORed with terminal-wide reverse video (DECSCNM).
        if attrs.reverse() == !reverse_video {
            std::mem::swap(&mut fg, &mut bg);
            bg_is_default = false;
        }

        (
            fg.to_linear(),
            bg.to_linear().mul_alpha(cfg.text_background_opacity),
            bg_is_default,
        )
    }

    /// Submit the recorded quads. Adapted verbatim from
    /// `TermWindow::call_draw_webgpu` in wezterm-gui/src/termwindow/render/draw.rs.
    fn draw_frame(&mut self) -> Result<()> {
        let webgpu = &self.webgpu;
        let render_state = &self.render_state;

        let output = webgpu.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = webgpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let tex = render_state.glyph_cache.borrow().atlas.texture();
        let tex = tex.downcast_ref::<WebGpuTexture>().unwrap();
        let texture_view = tex.create_view(&wgpu::TextureViewDescriptor::default());

        let texture_linear_bind_group =
            webgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &webgpu.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&webgpu.texture_linear_sampler),
                    },
                ],
                label: Some("linear bind group"),
            });

        let texture_nearest_bind_group =
            webgpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &webgpu.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&webgpu.texture_nearest_sampler),
                    },
                ],
                label: Some("nearest bind group"),
            });

        let foreground_text_hsb = [1.0, 1.0, 1.0];
        let projection = euclid::Transform3D::<f32, f32, f32>::ortho(
            -(self.width as f32) / 2.0,
            self.width as f32 / 2.0,
            self.height as f32 / 2.0,
            -(self.height as f32) / 2.0,
            -1.0,
            1.0,
        )
        .to_arrays_transposed();

        let mut cleared = false;
        for layer in render_state.layers.borrow().iter() {
            for idx in 0..3 {
                let vb = &layer.vb.borrow()[idx];
                let (vc, index_count) = vb.vertex_index_count();
                if index_count > 0 {
                    let mut vertices = vb.current_vb_mut();
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if cleared {
                                    wgpu::LoadOp::Load
                                } else {
                                    wgpu::LoadOp::Clear(wgpu::Color {
                                        r: 0.0,
                                        g: 0.0,
                                        b: 0.0,
                                        a: 0.0,
                                    })
                                },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        occlusion_query_set: None,
                        timestamp_writes: None,
                    });
                    cleared = true;

                    let uniforms = webgpu.create_uniform(ShaderUniform {
                        foreground_text_hsb,
                        milliseconds: 0,
                        projection,
                    });

                    render_pass.set_pipeline(&webgpu.render_pipeline);
                    render_pass.set_bind_group(0, &uniforms, &[]);
                    render_pass.set_bind_group(1, &texture_linear_bind_group, &[]);
                    render_pass.set_bind_group(2, &texture_nearest_bind_group, &[]);
                    let vertex_buffer = vertices.webgpu_mut().recreate();
                    vertex_buffer.unmap();
                    render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                    render_pass.set_index_buffer(
                        vb.indices.webgpu().slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    debug_assert!(vc > 0 || index_count == 0);
                    render_pass.draw_indexed(0..index_count as _, 0, 0..1);
                }
                vb.next_index();
            }
        }

        webgpu.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}
