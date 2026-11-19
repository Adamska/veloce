use veloce::player::{DEMO_LOCATION, Playlist, PlaylistItem, RepeatMode};

fn playlist(count: usize) -> Playlist {
    let mut playlist = Playlist::with_seed(42);
    playlist.replace(
        (0..count)
            .map(|index| format!("/media/clip-{index}.mkv"))
            .collect(),
    );
    playlist
}

#[test]
fn titles_come_from_file_names_and_urls() {
    assert_eq!(
        PlaylistItem::new("/a/b/Movie.mkv".into()).title,
        "Movie.mkv"
    );
    assert_eq!(
        PlaylistItem::new("https://host/live/stream.m3u8?token=1".into()).title,
        "stream.m3u8"
    );
    assert_eq!(
        PlaylistItem::new(DEMO_LOCATION.into()).title,
        veloce::demo::synthetic_source::TITLE
    );
}

#[test]
fn advances_in_order_and_stops_at_the_end() {
    let mut playlist = playlist(3);
    let mut visited = Vec::new();
    while let Some(index) = playlist.advance(true) {
        visited.push(index);
    }
    assert_eq!(visited, [0, 1, 2]);
    assert_eq!(playlist.retreat(), Some(1));
    assert_eq!(
        playlist.current_item().map(|item| item.title.as_str()),
        Some("clip-1.mkv")
    );
}

#[test]
fn repeat_modes_wrap_or_hold() {
    let mut playlist = playlist(2);
    playlist.repeat = RepeatMode::All;
    playlist.select(1);
    assert_eq!(playlist.advance(true), Some(0));
    playlist.repeat = RepeatMode::One;
    assert_eq!(playlist.advance(true), Some(0));
    assert_eq!(playlist.advance(false), Some(1));
}

#[test]
fn shuffle_visits_every_item_exactly_once() {
    let mut playlist = playlist(8);
    playlist.select(3);
    playlist.set_shuffle(true);
    let mut visited = vec![3];
    while let Some(index) = playlist.advance(true) {
        visited.push(index);
    }
    visited.sort_unstable();
    assert_eq!(visited, (0..8).collect::<Vec<_>>());
}

#[test]
fn removing_items_keeps_the_current_selection_stable() {
    let mut playlist = playlist(4);
    playlist.select(2);
    playlist.remove(0);
    assert_eq!(
        playlist.current_item().map(|i| i.title.as_str()),
        Some("clip-2.mkv")
    );
    playlist.remove(1);
    assert_eq!(playlist.current(), None);
    assert_eq!(playlist.items().len(), 2);
}
