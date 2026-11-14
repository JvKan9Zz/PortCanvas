//! Local, background-only material passes. All calls require Slint's current GL context.
use anyhow::{bail, ensure, Result};
use glow::HasContext;
use slint::{BorrowedOpenGLTextureBuilder as ImageBuilder, BorrowedOpenGLTextureOrigin as Origin};

const VERTEX: &str = "#version 330 core\nout vec2 uv;\nvoid main(){uv=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(uv*2.0-1.0,0,1);}";
pub struct Surface {
    pub size: (u32, u32),
    pub crop: [f32; 4],
    pub radius: f32,
}
struct Target {
    texture: glow::Texture,
    size: (u32, u32),
    crop: Option<[f32; 4]>,
    radius: f32,
}
pub struct Gpu {
    gl: glow::Context,
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
    source: Option<glow::Texture>,
    framebuffer: Option<glow::Framebuffer>,
    targets: Vec<Target>,
    source_size: (u32, u32),
    parameters: Option<(i32, f32, [f32; 3])>,
    pub uploads: u64,
    pub passes: u64,
    pub driver: String,
}

/// Restore the renderer's complete affected state on success and every error path.
struct State<'a> {
    gl: &'a glow::Context,
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
    draw: Option<glow::Framebuffer>,
    read: Option<glow::Framebuffer>,
    active: u32,
    texture: Option<glow::Texture>,
    unpack: Option<glow::Buffer>,
    pack: Option<glow::Buffer>,
    viewport: [i32; 4],
    toggles: [(u32, bool); 4],
    pixel_store: [(u32, i32); 8],
}
impl<'a> State<'a> {
    unsafe fn save(gl: &'a glow::Context) -> Self {
        let active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        gl.active_texture(glow::TEXTURE0);
        let mut viewport = [0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        Self {
            gl,
            active,
            viewport,
            program: gl.get_parameter_program(glow::CURRENT_PROGRAM),
            vao: gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
            draw: gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
            read: gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
            texture: gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
            unpack: gl.get_parameter_buffer(glow::PIXEL_UNPACK_BUFFER_BINDING),
            pack: gl.get_parameter_buffer(glow::PIXEL_PACK_BUFFER_BINDING),
            toggles: [
                glow::SCISSOR_TEST,
                glow::BLEND,
                glow::DEPTH_TEST,
                glow::CULL_FACE,
            ]
            .map(|p| (p, gl.is_enabled(p))),
            pixel_store: [
                glow::UNPACK_ALIGNMENT,
                glow::UNPACK_ROW_LENGTH,
                glow::UNPACK_SKIP_PIXELS,
                glow::UNPACK_SKIP_ROWS,
                glow::PACK_ALIGNMENT,
                glow::PACK_ROW_LENGTH,
                glow::PACK_SKIP_PIXELS,
                glow::PACK_SKIP_ROWS,
            ]
            .map(|p| (p, gl.get_parameter_i32(p))),
        }
    }
}
impl Drop for State<'_> {
    fn drop(&mut self) {
        unsafe {
            let gl = self.gl;
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read);
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vao);
            gl.bind_texture(glow::TEXTURE_2D, self.texture);
            gl.active_texture(self.active);
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, self.unpack);
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, self.pack);
            for (p, value) in self.pixel_store {
                gl.pixel_store_i32(p, value);
            }
            for (p, enabled) in self.toggles {
                if enabled {
                    gl.enable(p);
                } else {
                    gl.disable(p);
                }
            }
        }
    }
}

/// Downsample the completed window on the GPU before reading pixels. Called only
/// from AfterRendering, while the modal is still transparent and before present.
pub unsafe fn capture_window(
    api: &slint::GraphicsAPI,
    size: (u32, u32),
    limit: u32,
) -> Result<(image::RgbaImage, f32)> {
    let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
        bail!("Native OpenGL snapshot unavailable")
    };
    ensure!(size.0 > 0 && size.1 > 0, "empty window snapshot");
    let gl = glow::Context::from_loader_function(|name| {
        get_proc_address(&std::ffi::CString::new(name).unwrap())
    });
    let saved = State::save(&gl);
    let ratio = (limit as f32 / size.0 as f32).min(640.0 / size.1 as f32).min(1.0);
    let (w,h) = ((size.0 as f32*ratio).round().max(1.0) as u32,
                 (size.1 as f32*ratio).round().max(1.0) as u32);
    let texture = gl.create_texture().map_err(anyhow::Error::msg)?;
    let framebuffer = match gl.create_framebuffer() {
        Ok(value) => value,
        Err(error) => { gl.delete_texture(texture); bail!(error); }
    };
    let result = (|| {
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
        gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(glow::TEXTURE_2D,0,glow::RGBA8 as i32,w as i32,h as i32,0,
            glow::RGBA,glow::UNSIGNED_BYTE,glow::PixelUnpackData::Slice(None));
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER,Some(framebuffer));
        gl.framebuffer_texture_2d(glow::DRAW_FRAMEBUFFER,glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,Some(texture),0);
        ensure!(gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER)==glow::FRAMEBUFFER_COMPLETE,
            "snapshot framebuffer incomplete");
        gl.disable(glow::SCISSOR_TEST);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER,saved.draw);
        gl.blit_framebuffer(0,0,size.0 as i32,size.1 as i32,0,0,w as i32,h as i32,
            glow::COLOR_BUFFER_BIT,glow::LINEAR);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER,Some(framebuffer));
        gl.pixel_store_i32(glow::PACK_ALIGNMENT,1);
        gl.pixel_store_i32(glow::PACK_ROW_LENGTH,0);
        gl.pixel_store_i32(glow::PACK_SKIP_PIXELS,0);
        gl.pixel_store_i32(glow::PACK_SKIP_ROWS,0);
        let mut pixels = image::RgbaImage::new(w,h);
        gl.read_pixels(0,0,w as i32,h as i32,glow::RGBA,glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(pixels.as_mut())));
        ensure!(gl.get_error()==glow::NO_ERROR,"GPU snapshot readback failed");
        Ok((pixels,ratio))
    })();
    gl.delete_framebuffer(framebuffer);
    gl.delete_texture(texture);
    drop(saved);
    result
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
    pub unsafe fn new(gl: glow::Context) -> Result<Self> {
        ensure!(
            if gl.version().is_embedded {
                gl.version().major >= 3
            } else {
                gl.version().major > 3 || gl.version().major == 3 && gl.version().minor >= 3
            },
            "OpenGL ES 3.0 or desktop OpenGL 3.3 required"
        );
        let mut result = Self {
            gl,
            program: None,
            vao: None,
            source: None,
            framebuffer: None,
            targets: Vec::new(),
            source_size: (0, 0),
            parameters: None,
            uploads: 0,
            passes: 0,
            driver: String::new(),
        };
        if let Err(error) = result.setup() {
            result.destroy();
            return Err(error);
        }
        Ok(result)
    }
    unsafe fn setup(&mut self) -> Result<()> {
        let gl = &self.gl;
        let _state = State::save(gl);
        let vertex = shader(gl, glow::VERTEX_SHADER, VERTEX)?;
        let fragment = match shader(gl, glow::FRAGMENT_SHADER, include_str!("material.frag")) {
            Ok(value) => value,
            Err(error) => {
                gl.delete_shader(vertex);
                return Err(error);
            }
        };
        let program = gl.create_program().map_err(anyhow::Error::msg);
        if let Ok(program) = program {
            self.program = Some(program);
            gl.attach_shader(program, vertex);
            gl.attach_shader(program, fragment);
            gl.link_program(program);
            gl.detach_shader(program, vertex);
            gl.detach_shader(program, fragment);
        }
        gl.delete_shader(vertex);
        gl.delete_shader(fragment);
        let program = program?;
        ensure!(
            gl.get_program_link_status(program),
            "{}",
            gl.get_program_info_log(program)
        );
        self.vao = Some(gl.create_vertex_array().map_err(anyhow::Error::msg)?);
        self.framebuffer = Some(gl.create_framebuffer().map_err(anyhow::Error::msg)?);
        self.source = Some(gl.create_texture().map_err(anyhow::Error::msg)?);
        self.texture(self.source.unwrap());
        self.driver = format!(
            "{} / {} / {}",
            gl.get_parameter_string(glow::VENDOR),
            gl.get_parameter_string(glow::RENDERER),
            gl.get_parameter_string(glow::VERSION)
        );
        Ok(())
    }
    unsafe fn texture(&self, texture: glow::Texture) {
        let gl = &self.gl;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        for (p, value) in [
            (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
            (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, value as i32);
        }
    }
    pub unsafe fn draw(
        &mut self,
        frame: Option<(u32, u32, &[u8])>,
        surfaces: &[Surface],
        quality: i32,
        opacity: f32,
        tint: [f32; 3],
    ) -> Result<(slint::Image, Vec<slint::Image>)> {
        let redraw_all=frame.is_some()||self.parameters!=Some((quality,opacity,tint));
        let gl = &self.gl;
        let _state = State::save(gl);
        gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
        for (p, value) in [
            (glow::UNPACK_ALIGNMENT, 1),
            (glow::UNPACK_ROW_LENGTH, 0),
            (glow::UNPACK_SKIP_PIXELS, 0),
            (glow::UNPACK_SKIP_ROWS, 0),
        ] {
            gl.pixel_store_i32(p, value);
        }
        gl.bind_texture(glow::TEXTURE_2D, self.source);
        if let Some((width, height, rgba)) = frame {
            ensure!(
                rgba.len() == width as usize * height as usize * 4,
                "Invalid material source"
            );
            if self.source_size == (width, height) {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(rgba)),
                );
            } else {
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
            }
            self.uploads += 1;
        }
        while self.targets.len() > surfaces.len() {
            gl.delete_texture(self.targets.pop().unwrap().texture);
        }
        while self.targets.len() < surfaces.len() {
            let texture = gl.create_texture().map_err(anyhow::Error::msg)?;
            self.targets.push(Target {
                texture,
                size: (0, 0),
                crop: None,
                radius: -1.0,
            });
            self.texture(texture);
        }
        gl.bind_framebuffer(glow::FRAMEBUFFER, self.framebuffer);
        for flag in [
            glow::SCISSOR_TEST,
            glow::BLEND,
            glow::DEPTH_TEST,
            glow::CULL_FACE,
        ] {
            gl.disable(flag);
        }
        let program = self.program.unwrap();
        gl.use_program(self.program);
        gl.bind_vertex_array(self.vao);
        gl.uniform_1_i32(gl.get_uniform_location(program, "source").as_ref(), 0);
        gl.uniform_1_i32(
            gl.get_uniform_location(program, "quality").as_ref(),
            quality,
        );
        gl.uniform_1_f32(
            gl.get_uniform_location(program, "opacity").as_ref(),
            opacity / 100.0,
        );
        gl.uniform_3_f32_slice(gl.get_uniform_location(program, "tint").as_ref(), &tint);
        gl.uniform_2_f32(
            gl.get_uniform_location(program, "source_size").as_ref(),
            self.source_size.0 as f32,
            self.source_size.1 as f32,
        );
        let mut images = Vec::with_capacity(surfaces.len());
        for (target, surface) in self.targets.iter_mut().zip(surfaces) {
            // A moving overlay does not invalidate unrelated static islands.
            if !redraw_all&&target.size==surface.size&&target.crop==Some(surface.crop)&&target.radius==surface.radius{
                images.push(ImageBuilder::new_gl_2d_rgba_texture(target.texture.0,target.size.into()).origin(Origin::BottomLeft).build());
                continue;
            }
            if target.size != surface.size {
                gl.bind_texture(glow::TEXTURE_2D, Some(target.texture));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    surface.size.0 as i32,
                    surface.size.1 as i32,
                    0,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                target.size = surface.size;
            }
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(target.texture),
                0,
            );
            ensure!(
                gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE,
                "Incomplete material framebuffer"
            );
            gl.bind_texture(glow::TEXTURE_2D, self.source);
            gl.viewport(0, 0, surface.size.0 as i32, surface.size.1 as i32);
            gl.uniform_4_f32_slice(
                gl.get_uniform_location(program, "crop").as_ref(),
                &surface.crop,
            );
            gl.uniform_2_f32(
                gl.get_uniform_location(program, "surface_size").as_ref(),
                surface.size.0 as f32,
                surface.size.1 as f32,
            );
            gl.uniform_1_f32(gl.get_uniform_location(program,"corner_radius").as_ref(),surface.radius);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            target.crop=Some(surface.crop);
            target.radius=surface.radius;
            self.passes += 1;
            images.push(
                ImageBuilder::new_gl_2d_rgba_texture(target.texture.0, target.size.into())
                    .origin(Origin::BottomLeft)
                    .build(),
            );
        }
        ensure!(
            gl.get_error() == glow::NO_ERROR,
            "OpenGL material operation failed"
        );
        self.parameters=Some((quality,opacity,tint));
        Ok((
            ImageBuilder::new_gl_2d_rgba_texture(self.source.unwrap().0, self.source_size.into())
                .origin(Origin::TopLeft)
                .build(),
            images,
        ))
    }
    pub unsafe fn destroy(self) {
        for target in self.targets {
            self.gl.delete_texture(target.texture);
        }
        if let Some(v) = self.framebuffer {
            self.gl.delete_framebuffer(v);
        }
        if let Some(v) = self.source {
            self.gl.delete_texture(v);
        }
        if let Some(v) = self.vao {
            self.gl.delete_vertex_array(v);
        }
        if let Some(v) = self.program {
            self.gl.delete_program(v);
        }
    }
}
