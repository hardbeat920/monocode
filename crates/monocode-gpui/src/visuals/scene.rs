//! Small perspective scene renderer. Geometry stays in Rust and paints through GPUI paths.
use monocode_core::visual::{Camera, Geometry, SceneObject, ThemeColor};

type Vec3 = [f32; 3];
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: Vec3) -> Vec3 {
    mul(a, 1. / dot(a, a).sqrt().max(0.00001))
}
fn rotate(mut p: Vec3, r: Vec3) -> Vec3 {
    let (s, c) = r[0].sin_cos();
    p = [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c];
    let (s, c) = r[1].sin_cos();
    p = [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c];
    let (s, c) = r[2].sin_cos();
    [p[0] * c - p[1] * s, p[0] * s + p[1] * c, p[2]]
}
fn transform(p: Vec3, o: &SceneObject) -> Vec3 {
    add(
        rotate(
            [p[0] * o.scale[0], p[1] * o.scale[1], p[2] * o.scale[2]],
            o.rotation,
        ),
        o.position,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
}
impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: 0.,
            pitch: 0.,
            zoom: 1.,
        }
    }
}

pub(super) struct Face {
    pub points: Vec<[f32; 2]>,
    pub depth: f32,
    pub color: ThemeColor,
    pub light: f32,
}
pub(super) struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub depth: f32,
    pub width: f32,
    pub color: ThemeColor,
}
pub(super) struct Projection {
    pub faces: Vec<Face>,
    pub strokes: Vec<Stroke>,
    pub order: Vec<Surface>,
}
pub(super) enum Surface {
    Face(usize),
    Stroke(usize),
}

struct Lens {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    focal: f32,
    width: f32,
    height: f32,
}
impl Lens {
    fn new(camera: &Camera, orbit: Orbit, width: f32, height: f32) -> Self {
        let eye = add(
            camera.target,
            mul(
                rotate(sub(camera.eye, camera.target), [orbit.pitch, orbit.yaw, 0.]),
                orbit.zoom,
            ),
        );
        let forward = unit(sub(camera.target, eye));
        let world_up = if forward[1].abs() > 0.995 {
            [0., 0., 1.]
        } else {
            [0., 1., 0.]
        };
        let right = unit(cross(forward, world_up));
        let up = cross(right, forward);
        Self {
            eye,
            right,
            up,
            forward,
            focal: height / (2. * (camera.fov.to_radians() / 2.).tan()),
            width,
            height,
        }
    }
    fn depth(&self, p: Vec3) -> f32 {
        dot(sub(p, self.eye), self.forward)
    }
    fn project(&self, p: Vec3) -> [f32; 2] {
        let v = sub(p, self.eye);
        let z = dot(v, self.forward).max(0.01);
        [
            self.width / 2. + dot(v, self.right) * self.focal / z,
            self.height / 2. - dot(v, self.up) * self.focal / z,
        ]
    }
    // Clip to the near plane before perspective division, including intersecting faces.
    fn clip(&self, points: &[Vec3]) -> Vec<Vec3> {
        let mut out = Vec::new();
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            let da = self.depth(a);
            let db = self.depth(b);
            if da >= 0.01 {
                out.push(a);
            }
            if (da >= 0.01) != (db >= 0.01) {
                out.push(add(a, mul(sub(b, a), (0.01 - da) / (db - da))));
            }
        }
        out
    }
}

fn triangles(g: &Geometry) -> Vec<[Vec3; 3]> {
    match g {
        Geometry::Box => {
            let vertices = [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ];
            [
                [0, 3, 2],
                [0, 2, 1],
                [4, 5, 6],
                [4, 6, 7],
                [0, 4, 7],
                [0, 7, 3],
                [1, 2, 6],
                [1, 6, 5],
                [0, 1, 5],
                [0, 5, 4],
                [3, 7, 6],
                [3, 6, 2],
            ]
            .into_iter()
            .map(|i| i.map(|n| vertices[n]))
            .collect()
        }
        Geometry::Plane => vec![
            [[-0.5, 0., -0.5], [0.5, 0., 0.5], [0.5, 0., -0.5]],
            [[-0.5, 0., -0.5], [-0.5, 0., 0.5], [0.5, 0., 0.5]],
        ],
        Geometry::Sphere => {
            let mut out = Vec::new();
            let p = |a: f32, b: f32| {
                [
                    0.5 * a.sin() * b.cos(),
                    0.5 * a.cos(),
                    0.5 * a.sin() * b.sin(),
                ]
            };
            for lat in 0..16 {
                for lon in 0..24 {
                    let a = lat as f32 * std::f32::consts::PI / 16.;
                    let b = lon as f32 * std::f32::consts::TAU / 24.;
                    let aa = (lat + 1) as f32 * std::f32::consts::PI / 16.;
                    let bb = (lon + 1) as f32 * std::f32::consts::TAU / 24.;
                    for mut t in [
                        [p(a, b), p(aa, b), p(aa, bb)],
                        [p(a, b), p(aa, bb), p(a, bb)],
                    ] {
                        if dot(
                            cross(sub(t[1], t[0]), sub(t[2], t[0])),
                            add(add(t[0], t[1]), t[2]),
                        ) < 0.
                        {
                            t.swap(1, 2);
                        }
                        out.push(t);
                    }
                }
            }
            out
        }
        Geometry::Cylinder | Geometry::Cone => {
            let mut out = Vec::new();
            let top_radius = if matches!(g, Geometry::Cone) { 0. } else { 0.5 };
            for i in 0..32 {
                let a = i as f32 * std::f32::consts::TAU / 32.;
                let b = (i + 1) as f32 * std::f32::consts::TAU / 32.;
                let l = [0.5 * a.cos(), -0.5, 0.5 * a.sin()];
                let r = [0.5 * b.cos(), -0.5, 0.5 * b.sin()];
                let tl = [top_radius * a.cos(), 0.5, top_radius * a.sin()];
                let tr = [top_radius * b.cos(), 0.5, top_radius * b.sin()];
                out.extend([
                    [l, tl, r],
                    [r, tl, tr],
                    [[0., -0.5, 0.], l, r],
                    [[0., 0.5, 0.], tr, tl],
                ]);
            }
            out
        }
        Geometry::Mesh { vertices, indices } => indices
            .iter()
            .filter_map(|i| {
                Some([
                    *vertices.get(i[0])?,
                    *vertices.get(i[1])?,
                    *vertices.get(i[2])?,
                ])
            })
            .collect(),
        Geometry::Line { .. } => Vec::new(),
    }
}

pub(super) fn project(
    objects: &[SceneObject],
    camera: &Camera,
    orbit: Orbit,
    width: f32,
    height: f32,
) -> Projection {
    let lens = Lens::new(camera, orbit, width, height);
    let mut faces = Vec::new();
    let mut strokes = Vec::new();
    let mut triangle_budget = 24_000usize;
    let light = unit([-0.4, 0.8, 0.6]);
    for object in objects {
        if let Geometry::Line { points, width } = &object.geometry {
            // Clip each segment separately; do not bridge segments hidden behind the camera.
            for segment in points.windows(2) {
                let points = lens.clip(
                    &segment
                        .iter()
                        .map(|p| transform(*p, object))
                        .collect::<Vec<_>>(),
                );
                if points.len() >= 2 {
                    strokes.push(Stroke {
                        depth: points.iter().map(|p| lens.depth(*p)).sum::<f32>()
                            / points.len() as f32,
                        points: points.iter().take(2).map(|p| lens.project(*p)).collect(),
                        width: *width,
                        color: object.color,
                    });
                }
            }
            continue;
        }
        for triangle in triangles(&object.geometry) {
            if triangle_budget == 0 {
                break;
            }
            triangle_budget -= 1;
            let triangle = triangle.map(|p| transform(p, object));
            let normal = cross(sub(triangle[1], triangle[0]), sub(triangle[2], triangle[0]));
            if dot(normal, normal) < 0.00000001 {
                continue;
            }
            // Meshes and planes may intentionally be open/two-sided.
            if !matches!(object.geometry, Geometry::Plane | Geometry::Mesh { .. })
                && dot(normal, sub(lens.eye, triangle[0])) <= 0.
            {
                continue;
            }
            let points = lens.clip(&triangle);
            if points.len() < 3 {
                continue;
            }
            let depth = points.iter().map(|p| lens.depth(*p)).sum::<f32>() / points.len() as f32;
            faces.push(Face {
                points: points.iter().map(|p| lens.project(*p)).collect(),
                depth,
                color: object.color,
                light: 0.3 + 0.7 * dot(unit(normal), light).max(0.),
            });
        }
    }
    faces.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    let mut order = faces
        .iter()
        .enumerate()
        .map(|(i, f)| (f.depth, Surface::Face(i)))
        .chain(
            strokes
                .iter()
                .enumerate()
                .map(|(i, s)| (s.depth, Surface::Stroke(i))),
        )
        .collect::<Vec<_>>();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    Projection {
        faces,
        strokes,
        order: order.into_iter().map(|(_, surface)| surface).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn camera() -> Camera {
        Camera {
            eye: [3., 2., 5.],
            target: [0., 0., 0.],
            fov: 45.,
        }
    }
    fn cube() -> SceneObject {
        SceneObject {
            geometry: Geometry::Box,
            position: [0.; 3],
            rotation: [0.; 3],
            scale: [1.; 3],
            color: ThemeColor::Accent,
        }
    }
    #[test]
    fn perspective_center_and_zoom_are_stable() {
        let c = camera();
        let a = Lens::new(&c, Orbit::default(), 600., 400.);
        let center = a.project([0.; 3]);
        assert!((center[0] - 300.).abs() < 0.001 && (center[1] - 200.).abs() < 0.001);
        let b = Lens::new(
            &c,
            Orbit {
                zoom: 2.,
                ..Default::default()
            },
            600.,
            400.,
        );
        let pa = a.project([1., 0., 0.]);
        let pb = b.project([1., 0., 0.]);
        assert!((pb[0] - 300.).abs() < (pa[0] - 300.).abs());
    }
    #[test]
    fn visible_faces_are_depth_sorted_and_orbit_changes_projection() {
        let a = project(&[cube()], &camera(), Orbit::default(), 600., 400.);
        assert_eq!(a.faces.len(), 6);
        assert!(a.faces.windows(2).all(|p| p[0].depth >= p[1].depth));
        assert!(
            a.faces
                .iter()
                .flat_map(|f| &f.points)
                .flatten()
                .all(|n| n.is_finite())
        );
        let b = project(
            &[cube()],
            &camera(),
            Orbit {
                yaw: 0.8,
                ..Default::default()
            },
            600.,
            400.,
        );
        assert_ne!(a.faces[0].points, b.faces[0].points);
    }
    #[test]
    fn near_plane_clips_intersecting_triangles_without_infinities() {
        let lens = Lens::new(
            &Camera {
                eye: [0., 0., 0.],
                target: [0., 0., -1.],
                fov: 45.,
            },
            Orbit::default(),
            600.,
            400.,
        );
        let points = lens.clip(&[[-1., 0., -1.], [1., 0., -1.], [0., 1., 1.]]);
        assert_eq!(points.len(), 4);
        assert!(points.iter().all(|p| lens.depth(*p) >= 0.0099));
        assert!(
            points
                .into_iter()
                .flat_map(|p| lens.project(p))
                .all(|n| n.is_finite())
        );
    }
}
