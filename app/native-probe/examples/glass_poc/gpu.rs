use anyhow::{bail, ensure, Result};
use glow::HasContext;
use slint::{BorrowedOpenGLTextureBuilder as ImageBuilder, BorrowedOpenGLTextureOrigin as Origin};
use std::time::Instant;
const VERTEX:&str="#version 330 core\nout vec2 uv;\nvoid main(){uv=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(uv*2.0-1.0,0,1);}";
const WIDTH: i32 = 1120;
const HEIGHT: i32 = 280;
pub struct Gpu {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    source: glow::Texture,
    surface: glow::Texture,
    framebuffer: glow::Framebuffer,
    source_size: (u32, u32),
    queries: Vec<(glow::Query, Option<i32>)>,
    pub gpu_samples: Vec<(i32, f64)>,
    pub submit_samples: Vec<(i32, f64)>,
    pub uploads: u64,
    pub draws: u64,
    pub driver: String,
    timer_disjoint: bool,
}
unsafe fn shader(gl: &glow::Context, kind: u32, source: &str) -> Result<glow::Shader> {
    let source = if gl.version().is_embedded {
        source.replacen(
            "#version 330 core",
            "#version 300 es\nprecision highp float;\nprecision highp int;",
            1,
        )
    } else {
        source.to_owned()
    };
    let shader = gl.create_shader(kind).map_err(anyhow::Error::msg)?;
    gl.shader_source(shader, &source);
    gl.compile_shader(shader);
    if !gl.get_shader_compile_status(shader) {
        let error = gl.get_shader_info_log(shader);
        gl.delete_shader(shader);
        bail!(error);
    }
    Ok(shader)
}
impl Gpu {
    pub fn timer_queries_supported(&self) -> bool {
        !self.queries.is_empty()
    }
    /// Called only while Slint's own OpenGL context is current.
    pub unsafe fn new(gl: glow::Context) -> Result<Self> {
        ensure!(
            if gl.version().is_embedded {
                gl.version().major >= 3
            } else {
                gl.version().major > 3 || gl.version().major == 3 && gl.version().minor >= 3
            },
            "OpenGL ES 3.0 or desktop OpenGL 3.3 required"
        );
        let timer_disjoint = gl.version().is_embedded
            && gl
                .supported_extensions()
                .contains("GL_EXT_disjoint_timer_query");
        let timers = !gl.version().is_embedded || timer_disjoint;
        let vertex = shader(&gl, glow::VERTEX_SHADER, VERTEX)?;
        let fragment = match shader(&gl, glow::FRAGMENT_SHADER, include_str!("material.frag")) {
            Ok(s) => s,
            Err(e) => {
                gl.delete_shader(vertex);
                return Err(e);
            }
        };
        let program = gl.create_program().map_err(anyhow::Error::msg)?;
        gl.attach_shader(program, vertex);
        gl.attach_shader(program, fragment);
        gl.link_program(program);
        gl.detach_shader(program, vertex);
        gl.detach_shader(program, fragment);
        gl.delete_shader(vertex);
        gl.delete_shader(fragment);
        if !gl.get_program_link_status(program) {
            let error = gl.get_program_info_log(program);
            gl.delete_program(program);
            bail!(error);
        }
        let vao = gl.create_vertex_array().map_err(anyhow::Error::msg)?;
        let source = gl.create_texture().map_err(anyhow::Error::msg)?;
        let surface = gl.create_texture().map_err(anyhow::Error::msg)?;
        let framebuffer = gl.create_framebuffer().map_err(anyhow::Error::msg)?;
        let old_texture = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
        let old_framebuffer = gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING);
        let old_read_framebuffer = gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
        for texture in [source, surface] {
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
        }
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            WIDTH,
            HEIGHT,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(surface),
            0,
        );
        let complete = gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, old_framebuffer);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, old_read_framebuffer);
        gl.bind_texture(glow::TEXTURE_2D, old_texture);
        let driver = format!(
            "{} / {} / {}",
            gl.get_parameter_string(glow::VENDOR),
            gl.get_parameter_string(glow::RENDERER),
            gl.get_parameter_string(glow::VERSION)
        );
        let queries = (0..if timers { 8 } else { 0 })
            .map(|_| {
                gl.create_query()
                    .map(|q| (q, None))
                    .map_err(anyhow::Error::msg)
            })
            .collect::<Result<_>>()?;
        let result = Self {
            gl,
            program,
            vao,
            source,
            surface,
            framebuffer,
            source_size: (0, 0),
            queries,
            gpu_samples: Vec::new(),
            submit_samples: Vec::new(),
            uploads: 0,
            draws: 0,
            driver,
            timer_disjoint,
        };
        if !complete {
            result.destroy();
            bail!("Incomplete material framebuffer");
        }
        Ok(result)
    }
    pub unsafe fn draw(
        &mut self,
        frame: Option<(u32, u32, &[u8])>,
        quality: i32,
        opacity: f32,
        crop: [f32; 4],
    ) -> Result<(slint::Image, slint::Image)> {
        let started = Instant::now();
        let gl = &self.gl;
        let disjoint = self.timer_disjoint && gl.get_parameter_i32(0x8FBB) != 0;
        for (query, mode) in &mut self.queries {
            if mode.is_some()
                && gl.get_query_parameter_u32(*query, glow::QUERY_RESULT_AVAILABLE) != 0
            {
                let elapsed = gl.get_query_parameter_u32(*query, glow::QUERY_RESULT);
                ensure!(
                    elapsed != u32::MAX,
                    "GPU query exceeded 32-bit duration range"
                );
                let mode = mode.take().unwrap();
                if !disjoint {
                    self.gpu_samples.push((mode, elapsed as f64 / 1000.0));
                }
            }
        }
        let old_program = gl.get_parameter_program(glow::CURRENT_PROGRAM);
        let old_vao = gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING);
        let old_draw = gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING);
        let old_read = gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
        let old_active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        gl.active_texture(glow::TEXTURE0);
        let old_texture = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
        let old_unpack = gl.get_parameter_buffer(glow::PIXEL_UNPACK_BUFFER_BINDING);
        let mut viewport = [0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        let toggles = [
            glow::SCISSOR_TEST,
            glow::BLEND,
            glow::DEPTH_TEST,
            glow::CULL_FACE,
        ]
        .map(|flag| (flag, gl.is_enabled(flag)));
        let old_alignment = gl.get_parameter_i32(glow::UNPACK_ALIGNMENT);
        let pixel_store = [
            glow::UNPACK_ROW_LENGTH,
            glow::UNPACK_SKIP_PIXELS,
            glow::UNPACK_SKIP_ROWS,
        ]
        .map(|p| (p, gl.get_parameter_i32(p)));
        gl.bind_texture(glow::TEXTURE_2D, Some(self.source));
        if let Some((width, height, rgba)) = frame {
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            for (parameter, _) in pixel_store {
                gl.pixel_store_i32(parameter, 0);
            }
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(rgba)),
            );
            self.source_size = (width, height);
            self.uploads += 1;
        }
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
        gl.viewport(0, 0, WIDTH, HEIGHT);
        for (flag, _) in toggles {
            gl.disable(flag);
        }
        gl.use_program(Some(self.program));
        gl.bind_vertex_array(Some(self.vao));
        gl.uniform_1_i32(gl.get_uniform_location(self.program, "source").as_ref(), 0);
        gl.uniform_1_i32(
            gl.get_uniform_location(self.program, "quality").as_ref(),
            quality,
        );
        gl.uniform_1_f32(
            gl.get_uniform_location(self.program, "opacity").as_ref(),
            opacity / 100.0,
        );
        gl.uniform_4_f32_slice(
            gl.get_uniform_location(self.program, "crop").as_ref(),
            &crop,
        );
        gl.uniform_2_f32(
            gl.get_uniform_location(self.program, "source_size")
                .as_ref(),
            self.source_size.0 as f32,
            self.source_size.1 as f32,
        );
        gl.uniform_2_f32(
            gl.get_uniform_location(self.program, "surface_size")
                .as_ref(),
            WIDTH as f32,
            HEIGHT as f32,
        );
        let query = self.queries.iter_mut().find(|(_, mode)| mode.is_none());
        if let Some((q, _)) = query.as_ref() {
            gl.begin_query(glow::TIME_ELAPSED, *q);
        }
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        if let Some((_, mode)) = query {
            gl.end_query(glow::TIME_ELAPSED);
            *mode = Some(quality);
        }
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, old_draw);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, old_read);
        gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
        gl.use_program(old_program);
        gl.bind_vertex_array(old_vao);
        gl.bind_texture(glow::TEXTURE_2D, old_texture);
        gl.active_texture(old_active);
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, old_unpack);
        gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, old_alignment);
        for (parameter, value) in pixel_store {
            gl.pixel_store_i32(parameter, value);
        }
        for (flag, enabled) in toggles {
            if enabled {
                gl.enable(flag);
            } else {
                gl.disable(flag);
            }
        }
        self.draws += 1;
        self.submit_samples
            .push((quality, started.elapsed().as_secs_f64() * 1e6));
        ensure!(
            gl.get_error() == glow::NO_ERROR,
            "OpenGL material operation failed"
        );
        Ok((
            ImageBuilder::new_gl_2d_rgba_texture(self.source.0, self.source_size.into())
                .origin(Origin::TopLeft)
                .build(),
            ImageBuilder::new_gl_2d_rgba_texture(
                self.surface.0,
                (WIDTH as u32, HEIGHT as u32).into(),
            )
            .origin(Origin::BottomLeft)
            .build(),
        ))
    }
    pub unsafe fn destroy(self) {
        for (query, _) in self.queries {
            self.gl.delete_query(query);
        }
        self.gl.delete_framebuffer(self.framebuffer);
        self.gl.delete_texture(self.surface);
        self.gl.delete_texture(self.source);
        self.gl.delete_vertex_array(self.vao);
        self.gl.delete_program(self.program);
    }
}
