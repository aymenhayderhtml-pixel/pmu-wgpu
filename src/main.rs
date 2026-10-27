mod ai;
mod ally;
mod animation;
mod app;
mod camera;
mod camp;
mod collision;
mod combat;
mod debug_lines;
mod effects;
mod enemy;
mod fairy;
mod font;
mod frustum;
mod game_flow;
mod gpu_model;
mod hud;
mod hud_life;
mod hud_menu;
mod hud_squad;
mod input;
mod life;
mod menu;
mod mesh;
mod mipmap;
mod model;
mod nav;
mod offline;
mod pickups;
mod player_anim;
mod post;
mod projectiles;
mod renderer;
mod save;
mod settings;
mod shadow;
mod skill_inv;
mod skill_menu;
mod skills;
mod sky;
mod squad;
mod stats;
mod status;
mod training;
mod traits;
mod ui_text;
mod waves;
mod world;

use app::App;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().expect("Failed to create winit EventLoop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    event_loop
        .run_app(&mut app)
        .expect("EventLoop terminated with an error");
}
