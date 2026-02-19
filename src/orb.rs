use crossterm::{
    cursor, execute,
    style::{Color, Print, SetForegroundColor},
    terminal,
};
use rand::Rng;
use std::io::{Write, stdout};
use std::thread;
use std::time::Duration;
use std::{f32::consts::PI, thread::JoinHandle};

use crate::state::{LifeCycleState, LlmState, StateHandle};

const FPS: u64 = 30;

struct Point3D {
    x: f32,
    y: f32,
    z: f32,
}

pub struct OrbHandle {
    _handle: JoinHandle<()>,
}

pub fn spawn_orb_thread(state: StateHandle) -> OrbHandle {
    let handle = thread::spawn(move || {
        let _ = summon_orb(state);
    });

    OrbHandle { _handle: handle }
}

fn create_particles() -> Vec<Point3D> {
    let mut particles = Vec::new();

    // Create a cloud of particles in roughly spherical shape with some structure
    for i in 0..500 {
        let theta = (i as f32 * 2.4) % (2.0 * PI);
        let phi = (i as f32 * 1.618) % PI;
        let r = 2.0 + (i as f32 * 0.1).sin() * 0.4;

        particles.push(Point3D {
            x: r * phi.sin() * theta.cos(),
            y: r * phi.sin() * theta.sin(),
            z: r * phi.cos(),
        });
    }

    particles
}

fn rotate(p: &Point3D, rx: f32, ry: f32, rz: f32, multiplier: f32) -> Point3D {
    let rx = rx * multiplier;
    let ry = ry * multiplier;
    let rz = rz * multiplier;

    // Rotate around X
    let y1 = p.y * rx.cos() - p.z * rx.sin();
    let z1 = p.y * rx.sin() + p.z * rx.cos();

    // Rotate around Y
    let x2 = p.x * ry.cos() + z1 * ry.sin();
    let z2 = -p.x * ry.sin() + z1 * ry.cos();

    // Rotate around Z
    let x3 = x2 * rz.cos() - y1 * rz.sin();
    let y3 = x2 * rz.sin() + y1 * rz.cos();

    Point3D {
        x: x3,
        y: y3,
        z: z2,
    }
}

fn get_color(z: f32, intensity: f32, is_grey: bool) -> Color {
    if is_grey {
        return Color::Rgb {
            r: 90,
            g: 90,
            b: 90,
        };
    }

    let z_norm = (z + 2.0) / 4.0;

    if z_norm < 0.4 {
        Color::Rgb {
            r: 50,
            g: 180,
            b: 255,
        }
    } else if z_norm < 0.6 {
        Color::Rgb {
            r: 50,
            g: 100,
            b: 200,
        }
    } else {
        Color::Rgb {
            r: 0,
            g: (30.0 * intensity) as u8,
            b: (200.0 * intensity) as u8,
        }
    }
}

fn get_char(intensity: f32) -> char {
    match intensity {
        i if i > 0.8 => '#',
        i if i > 0.7 => 'x',
        i if i > 0.5 => '*',
        i if i > 0.3 => '+',
        _ => '.',
    }
}

fn summon_orb(state: StateHandle) -> anyhow::Result<()> {
    let mut stdout = stdout();

    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let particles = create_particles();
    let mut frame = 0.0;
    let mut speaking_modifier = 1.0_f32;
    let mut spin_multiplier = 1.0_f32;
    let mut rng = rand::rng();

    loop {
        let current_state = state.read();

        if current_state.life_cycle_state == LifeCycleState::ShuttingDown {
            break;
        }

        let (width, height) = {
            let (_width, _height) = terminal::size()?;
            (_width as usize, _height as usize)
        };

        let mut buffer = vec![' '; width * height];
        let mut z_buffer = vec![f32::NEG_INFINITY; width * height];

        // Morph factor
        let pulse = if current_state.llm_state == LlmState::RunningTts {
            speaking_modifier += rng.random_range(-0.3..0.3);

            if speaking_modifier > 1.5 {
                speaking_modifier -= rng.random_range(0.1..0.5);
            } else if speaking_modifier < -0.2 {
                speaking_modifier += rng.random_range(0.1..0.5);
            }

            speaking_modifier
        } else {
            (frame * 0.05_f32).sin()
        } * 0.3
            + 1.0;

        // Rotation angles
        let rx = frame * 0.01;
        let ry = frame * 0.015;
        let rz = frame * 0.008;

        spin_multiplier = (spin_multiplier
            + match current_state.llm_state {
                LlmState::AwaitingInput | LlmState::RunningTts => -2.0,
                LlmState::InitializingTts | LlmState::RunningInference => 0.2,
            })
        .clamp(1.0, 10.0);

        for particle in &particles {
            // Apply morphing
            let morphed = Point3D {
                x: particle.x * pulse,
                y: particle.y * pulse,
                z: particle.z * pulse,
            };

            let rotated = rotate(&morphed, rx, ry, rz, spin_multiplier);

            // Project to 2D (scale x by 2 for terminal aspect ratio)
            let scale = 10.0 / (rotated.z + 4.0);
            let x =
                ((rotated.x * scale * 2.0 + width as f32 / 2.0) as i32).clamp(0, width as i32 - 1);
            let y = ((rotated.y * scale + height as f32 / 2.0) as i32).clamp(0, height as i32 - 1);

            let idx = y as usize * width + x as usize;

            if rotated.z > z_buffer[idx] {
                z_buffer[idx] = rotated.z;
                let intensity = ((rotated.z + 2.0) / 4.0).clamp(0.0, 1.0);
                buffer[idx] = get_char(intensity);
            }
        }

        // Render
        execute!(stdout, cursor::Hide, cursor::MoveTo(0, 0))?;

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let c = buffer[idx];

                if c != ' ' {
                    let z = z_buffer[idx];
                    let intensity = ((z + 2.0) / 4.0).clamp(0.0, 1.0);
                    let color = get_color(
                        z,
                        intensity,
                        current_state.life_cycle_state != LifeCycleState::Running
                            || current_state.user_mute,
                    );
                    execute!(stdout, SetForegroundColor(color), Print(c))?;
                } else {
                    execute!(stdout, Print(' '))?;
                }
            }
        }

        if let Some((text, pos)) = current_state.text_input {
            let text_x = (width.saturating_sub(text.len())) / 2;
            execute!(
                stdout,
                cursor::MoveTo(text_x as u16, height as u16 - 2),
                SetForegroundColor(Color::White),
                Print(text),
                cursor::MoveTo(text_x as u16 + pos as u16, height as u16 - 2),
                cursor::Show,
            )?;
        }

        stdout.flush()?;

        if current_state.life_cycle_state == LifeCycleState::Running {
            frame += 1.0;
        }

        thread::sleep(Duration::from_millis(1000 / FPS));
    }

    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;

    Ok(())
}
