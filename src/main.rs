use minifb::{Key, Window, WindowOptions};


const WIDTH: usize = 640;
const HEIGHT: usize = 480;

fn main() {
    let mut buffer: Vec<u32> = vec![0xFFFFFF; WIDTH * HEIGHT];

    let mut window = Window::new("Smalltalk-80", WIDTH, HEIGHT, WindowOptions::default())
        .expect("Failed to create window");

    // Cap update rate to ~60fps
    window.set_target_fps(60);

    while window.is_open() && !window.is_key_down(Key::Escape) {
        window
            .update_with_buffer(&buffer, WIDTH, HEIGHT)
            .expect("Failed to update window");
    }
}
