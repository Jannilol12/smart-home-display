pub mod weather;

use embedded_graphics::draw_target::DrawTarget;
use epd_waveshare::color::Color;
use std::time::{Duration, Instant};

pub trait DisplayModule<D>
where
    D: DrawTarget<Color = Color>,
{
    fn name(&self) -> &'static str;
    fn update(&mut self) -> Result<(), ()>;
    fn render(&self, display: &mut D) -> Result<(), D::Error>;
    fn refresh_interval(&self) -> Duration;
    fn last_updated(&self) -> Option<Instant>;

    fn needs_update(&self) -> bool {
        match self.last_updated() {
            Some(last) => last.elapsed() >= self.refresh_interval(),
            None => true,
        }
    }
}
