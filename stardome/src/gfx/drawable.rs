// Drawables are managed by user
// Passed by mutable reference to program
// Then drawn

pub trait Drawable {
    // Perhaps some type equals error kind of thing
    // The jank is real
    fn draw(
        &mut self,
        g: &mut super::Graphics,
        c: &super::camera::Camera,
        s: na::Vector3<f32>,
        #[cfg(all(not(target_os = "emscripten"), not(feature = "gles")))] t: &mut Vec<(na::Vector3<f64>, u32, imgui::ImString)>,
    ) -> Result<(), std::ffi::NulError>;
}

// Future improvement: displacement/normal map
pub struct Planet {
    /// Equatorial radius
    pub r_equatorial: f64,
    /// Polar radius
    pub r_polar: f64,
    pub lighting: bool,
    pub texture: super::texture::Texture,
    pub atm: Option<Atmosphere>,
    pub clouds: Option<Clouds>,
    /// Transformation matrix, rotation and translation only (no scaling)
    pub tf: na::Matrix4<f64>,
}

// Need more atmosphere parameters
// Make sure to deal with different positions orientations scales properly
// TODO Use f32s/f64s consistently
pub struct Atmosphere {
    // Offset from point on ground
    // This way it fits with oblate spheroid
    pub offset: f64,
    pub sun_intensity: f64,
    pub scale_height_r: f64,
    pub scatter_coeff_r: na::Vector3<f32>,
    pub scale_height_m: f64,
    pub scatter_coeff_m: na::Vector3<f32>,
    pub asymmetry_m: f64,
}

// Should include cloud rotation
// Can be done by keeping track of elapsed time
// Shouldn't need a "state" per se
pub struct Clouds {
    // Offset from point on ground
    // This way it fits with oblate spheroid
    pub offset: f64,
    pub texture: super::texture::Texture,
}

impl Planet {
    pub fn mat64(&self, atm: Option<&Atmosphere>) -> na::Matrix4<f64> {
        if let Some(a) = atm {
            return self.tf
                * na::Matrix4::new_nonuniform_scaling(&na::Vector3::new(
                    self.r_equatorial + a.offset,
                    self.r_equatorial + a.offset,
                    self.r_polar + a.offset,
                ));
        }
        self.tf
            * na::Matrix4::new_nonuniform_scaling(&na::Vector3::new(
                self.r_equatorial,
                self.r_equatorial,
                self.r_polar,
            ))
    }

    pub fn mat32(&self, atm: Option<&Atmosphere>) -> na::Matrix4<f32> {
        na::convert(self.mat64(atm))
    }
}

impl Drawable for Planet {
    fn draw(
        &mut self,
        g: &mut super::Graphics,
        c: &super::camera::Camera,
        s: na::Vector3<f32>,
        #[cfg(all(not(target_os = "emscripten"), not(feature = "gles")))] _t: &mut Vec<(
            na::Vector3<f64>,
            u32,
            imgui::ImString,
        )>,
    ) -> Result<(), std::ffi::NulError> {
        let view = c.view_matrix();
        let projection = c.projection_matrix(g.aspect_ratio());
        g.progs[0].use_gl();
        println!("{:?}", g.progs);
        g.progs[0].set_mat4("model", &self.mat32(None))?;
        println!("{:?}", g.progs);
        g.progs[0].set_mat4("view", &view)?;
        g.progs[0].set_mat4("projection", &projection)?;
        let z = na::Vector3::zeros();
        g.progs[0].set_vec3("sun", if self.lighting { &s } else { &z })?;
        g.progs[0].set_vec3("cam_pos", &c.position)?;

        self.texture.bind(0);
        g.meshes[0].draw();
        g.progs[0].unuse_gl();

        if self.lighting && s != z {
            if let Some(a) = &self.atm {
                g.progs[3].use_gl();
                unsafe {
                    gl::Enable(gl::BLEND);
                    // TODO this is certainly wrong
                    gl::BlendFunc(gl::ONE, gl::ONE);
                }
                g.progs[3].set_mat4("model", &self.mat32(Some(&a)))?;
                g.progs[3].set_mat4("view", &view)?;
                g.progs[3].set_mat4("projection", &projection)?;
                g.progs[3].set_vec3("sun_dir", &s)?;
                g.progs[3].set_vec3("cam_pos", &(c.position * 1e6))?;
                g.progs[3].set_vec3("pos", &(c.position * 1e6))?; // TODO
                g.progs[3].set_float("Re", (self.r_equatorial * 1e6) as f32)?;
                g.progs[3].set_float("Ra", ((self.r_equatorial + a.offset) * 1e6) as f32)?;
                g.progs[3].set_float("Hr", a.scale_height_r as f32)?;
                g.progs[3].set_vec3("betaR", &a.scatter_coeff_r)?;
                g.progs[3].set_float("intensity", a.sun_intensity as f32)?;
                g.meshes[0].draw();
                unsafe { gl::Disable(gl::BLEND) }
                g.progs[3].unuse_gl();
            }
        }

        Ok(())
    }
}

// Consider a base transform
// That way less modifications are needed for say
// Following a planet around
// Little to no performance change
// TODO consider not keeping the Vec around
// Just take a reference, set it and be done with it
// That way there's no silly games with keeping it synced
// Although it means there is no way to modify, only overwrite
// I reckon do it
pub struct Points {
    pub color: u32, // RGBA
    pub width: f32,
    pub line: bool,
    // Private so GL stuff can be generated once per modification
    points: Vec<na::Vector3<f32>>,
    vbo: u32,
    vao: u32,
}

impl Points {
    pub fn new(color: u32, width: f32, line: bool, points: Vec<na::Vector3<f32>>) -> Self {
        let mut vbo: u32 = 0;
        let mut vao: u32 = 0;
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
        }
        let mut s = Self {
            color,
            width,
            line,
            points,
            vbo,
            vao,
        };
        s.update_points();
        s
    }

    // TODO don't let the Vec become len 0
    // OR IF YOU DO, DONT CAUSE ERRORS

    pub fn get_points(&self) -> &[na::Vector3<f32>] {
        self.points.as_slice()
    }

    pub fn modify_points<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Vec<na::Vector3<f32>>),
    {
        f(&mut self.points);
        self.update_points();
    }

    pub fn replace_points(&mut self, pts: Vec<na::Vector3<f32>>) -> Vec<na::Vector3<f32>> {
        let o = std::mem::replace(&mut self.points, pts);
        self.update_points();
        o
    }

    pub fn set_points(&mut self, pts: Vec<na::Vector3<f32>>) {
        self.points = pts;
        self.update_points();
    }

    fn update_points(&mut self) {
        if self.points.is_empty() {
            panic!("bruh");
        }
        unsafe {
            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);

            // I pray the layout of Vector3 is normal
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(self.points.as_slice()) as isize,
                self.points.as_ptr() as *const _,
                // Might be nice to be able to set this
                gl::DYNAMIC_DRAW,
            );

            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, 12, std::ptr::null());
            gl::EnableVertexAttribArray(0);

            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);
        }
    }
}

impl Drawable for Points {
    fn draw(
        &mut self,
        g: &mut super::Graphics,
        c: &super::camera::Camera,
        _s: na::Vector3<f32>,
        #[cfg(all(not(target_os = "emscripten"), not(feature = "gles")))] _t: &mut Vec<(
            na::Vector3<f64>,
            u32,
            imgui::ImString,
        )>,
    ) -> Result<(), std::ffi::NulError> {
        // TODO let user provide this
        let model: na::Matrix4<f32> = na::Matrix4::identity();
        let view = c.view_matrix();
        let projection = c.projection_matrix(g.aspect_ratio());
        // Wrong program for this
        g.progs[2].use_gl();
        g.progs[2].set_mat4("model", &model)?;
        g.progs[2].set_mat4("view", &view)?;
        g.progs[2].set_mat4("projection", &projection)?;
        let comp: [u8; 4] = unsafe { std::mem::transmute(self.color) };
        let v = 0xFF as f32;
        let color = na::Vector4::new(
            comp[3] as f32 / v,
            comp[2] as f32 / v,
            comp[1] as f32 / v,
            comp[0] as f32 / v,
        );
        g.progs[2].set_vec4("color", &color)?;
        unsafe {
            // TODO not working :(
            // Probably need to draw it as some kind of mesh
            // Not epic
            if self.line {
                //gl::LineWidth(self.width);
            } else {
                gl::PointSize(self.width);
            }
            gl::BindVertexArray(self.vao);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::DrawArrays(
                // TODO GL_LINES, GL_LINE_LOOP
                // TODO line styles eg stipple
                if self.line {
                    gl::LINE_STRIP
                } else {
                    gl::POINTS
                },
                0,
                self.points.len() as i32,
            );
            gl::Disable(gl::BLEND);
            gl::BindVertexArray(0);
        }
        g.progs[2].unuse_gl();
        Ok(())
    }
}

pub struct Text {
    pub position: na::Vector3<f64>,
    pub color: u32,
    pub text: String,
}

impl Drawable for Text {
    #[cfg(all(not(target_os = "emscripten"), not(feature = "gles")))]
    fn draw(
        &mut self,
        g: &mut super::Graphics,
        c: &super::camera::Camera,
        _s: na::Vector3<f32>,
        t: &mut Vec<(na::Vector3<f64>, u32, imgui::ImString)>,
    ) -> Result<(), std::ffi::NulError> {
        t.push((
            self.position.clone_owned(),
            self.color,
            imgui::ImString::new(self.text.clone()),
        ));
        Ok(())
    }

    #[cfg(any(target_os = "emscripten", feature = "gles"))]
    fn draw(
        &mut self,
        g: &mut super::Graphics,
        c: &super::camera::Camera,
        _s: na::Vector3<f32>,
    ) -> Result<(), std::ffi::NulError> {
        Ok(())
    }
}
