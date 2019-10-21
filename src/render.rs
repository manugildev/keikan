use std::f64;
use std::sync::Arc;
use rand::Rng;

use crate::structures::vec3::Vec3;
use crate::structures::ray::Ray;
use crate::structures::camera::Camera;
use crate::structures::material::Material;
use crate::structures::scene::Scene;
use crate::structures::cast_result::CastResult;
use crate::objects::traits::{ March, Trace };

// constants
const MAX_STEPS: u32 = 128;
const MAX_DEPTH: u32 = 512;
const MAX_BOUNCES: u32 = 3;
const SAMPLES: u32 = 16;
const EPSILON: f64 = 0.001;

// TODO: results are trapped and rays will self-intersect
fn hit_march(march: &Vec<Arc<dyn March>>, ray: Ray) -> CastResult {
    let sdf = |point: Vec3| {
        let mut min = f64::MAX;
        let mut mat = Material::blank();

        for object in march.iter() {
            let distance = object.march(point);

            if distance <= min {
                min = distance;
                mat = object.material();
            }
        }

        return (min, mat);
    };

    let normal = |p: Vec3| {
        Vec3::new(
            sdf(Vec3::new(p.x + EPSILON, p.y, p.z)).0 - sdf(Vec3::new(p.x - EPSILON, p.y, p.z)).0,
            sdf(Vec3::new(p.x, p.y + EPSILON, p.z)).0 - sdf(Vec3::new(p.x, p.y - EPSILON, p.z)).0,
            sdf(Vec3::new(p.x, p.y, p.z + EPSILON)).0 - sdf(Vec3::new(p.x, p.y, p.z - EPSILON)).0,
        ).unit()
    };

    let mut depth = 0.0;

    for step in 0..MAX_STEPS {
        let point = ray.point_at(&depth);
        let (distance, material) = sdf(point);

        if distance <= EPSILON {
            let normal = normal(point); // quick normal estimation

            // let mut mat = Material::blank();
            // mat.color = normal;

            return CastResult::new(true, depth, normal, material);
        }

        if distance >= MAX_DEPTH.into() {
            break;
        }

        depth += distance;
    }

    return CastResult::worst();
}

fn hit_trace(trace: &Vec<Arc<dyn Trace>>, ray: Ray) -> CastResult {
    // todo: cull behind camera

    let mut best = CastResult::worst();

    for object in trace.iter() {
        let (hit, distance, normal) = object.trace(ray);

        if hit && distance > EPSILON && (best.hit == false || distance <= best.distance) {
            best = CastResult::new(hit, distance, normal, object.material());
        }
    }

    return best;
}

fn cast_ray(scene: &Scene, ray: Ray) -> CastResult {
    let march = hit_march(&scene.march, ray);
    let trace = hit_trace(&scene.trace, ray);

    // nothing was hit, so return the sky
    if !march.hit && !trace.hit {
        return CastResult::worst();
    }

    if  trace.hit && !march.hit || trace.distance <= march.distance {
        return trace;
    }

    return march;
}

fn sample_sphere() -> Vec3 {
    let mut rng = rand::thread_rng();
    let mut point: Vec3 = Vec3::max();

    // sample point in unit cube, check if in unit sphere
    while point.length_squared() >= 1.0 {
        point = Vec3::new(
            rng.gen::<f64>() * 2.0 - 1.0,
            rng.gen::<f64>() * 2.0 - 1.0,
            rng.gen::<f64>() * 2.0 - 1.0,
         );
    }

    return point;
}

fn reflect(v: Vec3, n: Vec3) -> Vec3 {
    return v - 2.0 * v.dot(&n) * n;
}

fn fresnel(cosine: f64, ri: f64) -> f64 {
    let mut r0: f64 = (1.0 - ri)/(1.0 + ri);
    r0 = r0*r0;
    return r0 + (1.0-r0)*(1.0-cosine).powi(5);
}

fn refract(v: &Vec3, n: &Vec3, ni_over_nt: f64, refracted: &mut Vec3) -> bool {
    let uv: Vec3 = v.unit();
    let dt: f64 = uv.dot(&n);

    let discriminant: f64 = 1.0 - ni_over_nt*ni_over_nt*(1.0 - dt*dt);
    if discriminant > 0.0 {
        *refracted = ni_over_nt*(uv - *n*dt) - *n*((discriminant).sqrt());
        return true;
    } else {
        return false;
    }
}

// simplify
fn color(scene: &Scene, ray: Ray, bounce: u32, samples: u32) -> Vec3 {
    let (hit, distance, normal, material) = cast_ray(&scene, ray).unpack();

    // nothing hit, return the sky
    if !hit || bounce <= 0 {
        return material.color * material.emission;
    }

    let     position     = ray.point_at(&distance);
    let mut diffuse      = Vec3::new(0.0, 0.0, 0.0);
    let mut specular     = Vec3::new(0.0, 0.0, 0.0);
    let mut transmission = Vec3::new(0.0, 0.0, 0.0);

    // diffuse
    for _ in 0..samples {
        let scatter = Ray::through(position, position + normal + sample_sphere());
        let sample = color(&scene, scatter, (bounce - 1), 1); // only take one sample

        diffuse = diffuse + material.color * sample;
    }

    diffuse = diffuse / (samples as f64);

    //specular
    if material.roughness == 0.0 {
        let scatter = Ray::through(position, position + reflect(ray.direction, normal));
        specular = color(&scene, scatter, (bounce - 1), samples);
    } else {
        for _ in 0..samples {
            let scatter = Ray::through(
                position,
                position + reflect(ray.direction, normal) + sample_sphere() * material.roughness
            );

            let sample = color(&scene, scatter, (bounce - 1), (samples / 2).max(1));
            specular = specular + sample;
        }

        specular = specular / (samples as f64);
    }

    // TODO: transmission

    // combine the samples in a PBR manner
    return (
        (
            ( // for dielectric materials. TODO: fresnel blending
                (transmission *        material.transmission)  // mix transparent
              + (diffuse      * (1.0 - material.transmission)) // and diffuse
              + (specular     *        material.specular)      // with a specular layer on top
            )
          * (1.0 - material.metallic) // lerp with metal

          + ( // for metallic materials
                specular * material.color
            )
          * material.metallic
        )
      * (1.0 - material.emission).max(0.0) // modified lerp with emissive

      + ( // for emissive materials
          material.color * material.emission
        )
    );

    // let mut result = (transmission * material.transmission) + (diffuse * (1.0 - material.transmission)) ;
    // result = result + specular * material.specular;
    // result = result * (1.0 - material.metallic) + specular * material.color * material.metallic;
    // result = (material.color * material.emission) + (result * (1.0 - material.emission).max(0.0));
    //
    // return result;
}

// camera or scene
fn make_ray(origin: Vec3, fov: f64, ratio: f64, uv: [f64; 2]) -> Ray {
    // I apologize for this garbage
    let xy = [uv[0] - ratio * 0.5, uv[1] - 0.5];
    let z = 1.0 / (fov.to_radians() / 2.0).tan();
    return Ray::new(origin, (Vec3::new(xy[0], xy[1], -z)).unit());
}

pub fn render(scene: &Scene, uv: [f64; 2], resolution: [usize; 2]) -> Vec3 {
    // make ray
    let mut xy = [uv[0] / (resolution[0] as f64), uv[1] / (resolution[1] as f64)];
    xy[0] *= (resolution[0] as f64) / (resolution[1] as f64);

    let ray = make_ray(
        scene.camera.ray.origin,
        120.0, // standard fov
        (resolution[0] as f64) / (resolution[1] as f64),
        xy,
    );

    // cast ray
    return color(&scene, ray, MAX_BOUNCES, SAMPLES);
}
