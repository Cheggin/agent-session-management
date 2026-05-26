use ratatui::style::{Color, Modifier, Style};

use crate::Agent;

pub fn text() -> Color {
    Color::Rgb(205, 214, 244)
}

fn muted_color() -> Color {
    Color::Rgb(166, 173, 200)
}

fn dim_color() -> Color {
    Color::Rgb(108, 112, 134)
}

fn accent_color() -> Color {
    Color::Rgb(137, 180, 250)
}

fn border_color() -> Color {
    Color::Rgb(69, 71, 90)
}

fn green() -> Color {
    Color::Rgb(166, 227, 161)
}

fn orange() -> Color {
    Color::Rgb(250, 179, 135)
}

fn mauve() -> Color {
    Color::Rgb(203, 166, 247)
}

fn surface() -> Color {
    Color::Rgb(30, 30, 46)
}

pub fn text_style() -> Style {
    Style::default().fg(text())
}

pub fn muted() -> Style {
    Style::default().fg(muted_color())
}

pub fn dim() -> Style {
    Style::default().fg(dim_color())
}

pub fn accent() -> Style {
    Style::default()
        .fg(accent_color())
        .add_modifier(Modifier::BOLD)
}

pub fn border() -> Style {
    Style::default().fg(border_color())
}

pub fn selected() -> Style {
    Style::default().bg(Color::Rgb(45, 52, 66))
}

pub fn live() -> Style {
    Style::default().fg(green()).add_modifier(Modifier::BOLD)
}

pub fn live_maybe() -> Style {
    Style::default().fg(green()).add_modifier(Modifier::DIM)
}

pub fn branch() -> Style {
    Style::default().fg(mauve())
}

pub fn chip() -> Style {
    Style::default().fg(accent_color())
}

pub fn chip_pending_delete() -> Style {
    Style::default()
        .fg(surface())
        .bg(accent_color())
        .add_modifier(Modifier::BOLD)
}

pub fn toast() -> Style {
    Style::default()
        .fg(surface())
        .bg(green())
        .add_modifier(Modifier::BOLD)
}

pub fn spark() -> Style {
    Style::default().fg(accent_color())
}

pub fn agent(agent: &Agent) -> Style {
    match agent {
        Agent::Claude => Style::default().fg(orange()).add_modifier(Modifier::BOLD),
        Agent::Codex => Style::default()
            .fg(accent_color())
            .add_modifier(Modifier::BOLD),
    }
}
