//! The GPU layer: every triangle of a frame in one buffer, drawn in order.

use tradoor_render::Frame;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{OffscreenCanvas, WebGl2RenderingContext as GL, WebGlBuffer, WebGlProgram, WebGlShader, WebGlUniformLocation, WebGlVertexArrayObject};

const VERTEX: &str = "#version 300 es
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec4 a_color;
uniform vec2 u_size;
out vec4 v_color;
void main() {
  vec2 clip = a_pos / u_size * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
  v_color = a_color;
}";

const FRAGMENT: &str = "#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 color;
void main() { color = v_color; }";

pub struct Gl {
    canvas: OffscreenCanvas,
    gl: GL,
    program: WebGlProgram,
    vao: WebGlVertexArrayObject,
    positions: WebGlBuffer,
    colors: WebGlBuffer,
    u_size: Option<WebGlUniformLocation>,
    capacity: usize,
}

fn compile(gl: &GL, kind: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl.create_shader(kind).ok_or("could not create a shader")?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    if gl.get_shader_parameter(&shader, GL::COMPILE_STATUS).as_bool() != Some(true) {
        return Err(JsValue::from_str(&gl.get_shader_info_log(&shader).unwrap_or_default()));
    }
    Ok(shader)
}

impl Gl {
    pub fn new(canvas: OffscreenCanvas) -> Result<Self, JsValue> {
        let options = js_sys::Object::new();
        for (key, value) in [("antialias", true), ("alpha", false), ("depth", false), ("stencil", false), ("preserveDrawingBuffer", false)] {
            js_sys::Reflect::set(&options, &key.into(), &value.into())?;
        }
        let gl: GL = canvas.get_context_with_context_options("webgl2", &options)?.ok_or("WebGL2 is not available in this browser")?.dyn_into()?;

        let program = gl.create_program().ok_or("could not create a program")?;
        gl.attach_shader(&program, &compile(&gl, GL::VERTEX_SHADER, VERTEX)?);
        gl.attach_shader(&program, &compile(&gl, GL::FRAGMENT_SHADER, FRAGMENT)?);
        gl.link_program(&program);
        if gl.get_program_parameter(&program, GL::LINK_STATUS).as_bool() != Some(true) {
            return Err(JsValue::from_str(&gl.get_program_info_log(&program).unwrap_or_default()));
        }

        let vao = gl.create_vertex_array().ok_or("could not create a vertex array")?;
        gl.bind_vertex_array(Some(&vao));
        let positions = gl.create_buffer().ok_or("could not create a buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&positions));
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_with_i32(0, 2, GL::FLOAT, false, 0, 0);
        let colors = gl.create_buffer().ok_or("could not create a buffer")?;
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&colors));
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_with_i32(1, 4, GL::UNSIGNED_BYTE, true, 0, 0);

        gl.enable(GL::BLEND);
        gl.blend_func_separate(GL::SRC_ALPHA, GL::ONE_MINUS_SRC_ALPHA, GL::ONE, GL::ONE_MINUS_SRC_ALPHA);
        let u_size = gl.get_uniform_location(&program, "u_size");
        Ok(Self { canvas, gl, program, vao, positions, colors, u_size, capacity: 0 })
    }

    pub fn draw(&mut self, frame: &Frame) {
        let gl = &self.gl;
        let (w, h) = ((frame.width * frame.dpr).round().max(1.0) as u32, (frame.height * frame.dpr).round().max(1.0) as u32);
        if self.canvas.width() != w {
            self.canvas.set_width(w);
        }
        if self.canvas.height() != h {
            self.canvas.set_height(h);
        }
        gl.viewport(0, 0, w as i32, h as i32);
        gl.disable(GL::SCISSOR_TEST);
        let bg = frame.background;
        gl.clear_color(f32::from(bg.r) / 255.0, f32::from(bg.g) / 255.0, f32::from(bg.b) / 255.0, 1.0);
        gl.clear(GL::COLOR_BUFFER_BIT);
        let vertices = frame.vertex_count();
        if vertices == 0 {
            return;
        }

        gl.use_program(Some(&self.program));
        gl.bind_vertex_array(Some(&self.vao));
        let grow = vertices > self.capacity;
        if grow {
            self.capacity = vertices + vertices / 2;
        }
        // SAFETY: the views are handed straight to buffer calls that copy them,
        // with no allocation in between that could move WASM memory.
        unsafe {
            let positions = js_sys::Float32Array::view(&frame.positions);
            let colors = js_sys::Uint8Array::view(&frame.colors);
            gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.positions));
            if grow {
                gl.buffer_data_with_i32(GL::ARRAY_BUFFER, (self.capacity * 8) as i32, GL::DYNAMIC_DRAW);
            }
            gl.buffer_sub_data_with_i32_and_array_buffer_view(GL::ARRAY_BUFFER, 0, &positions);
            gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.colors));
            if grow {
                gl.buffer_data_with_i32(GL::ARRAY_BUFFER, (self.capacity * 4) as i32, GL::DYNAMIC_DRAW);
            }
            gl.buffer_sub_data_with_i32_and_array_buffer_view(GL::ARRAY_BUFFER, 0, &colors);
        }
        gl.uniform2f(self.u_size.as_ref(), frame.width, frame.height);

        for batch in frame.batches.iter().filter(|b| b.count > 0) {
            match batch.clip {
                Some(c) => {
                    gl.enable(GL::SCISSOR_TEST);
                    let x = (c.x * frame.dpr).floor();
                    let y = ((frame.height - c.bottom()) * frame.dpr).floor();
                    gl.scissor(x as i32, y as i32, (c.w * frame.dpr).ceil().max(0.0) as i32, (c.h * frame.dpr).ceil().max(0.0) as i32);
                }
                None => gl.disable(GL::SCISSOR_TEST),
            }
            gl.draw_arrays(GL::TRIANGLES, batch.first as i32, batch.count as i32);
        }
    }
}
