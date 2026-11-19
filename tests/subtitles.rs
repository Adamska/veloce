use veloce::subtitles::{
    SubtitleFormat,
    parser::{embedded_dialogue_text, parse, parse_timestamp, strip_markup},
};

#[test]
fn timestamps_accept_srt_vtt_and_ass_notation() {
    assert_eq!(parse_timestamp("00:01:02,500"), Some(62_500_000));
    assert_eq!(parse_timestamp("01:02.250"), Some(62_250_000));
    assert_eq!(parse_timestamp("1:00:00.00"), Some(3_600_000_000));
    assert_eq!(parse_timestamp("12"), None);
    assert_eq!(parse_timestamp("aa:bb:cc"), None);
}

#[test]
fn srt_cues_keep_line_breaks_and_drop_markup() {
    let content = "\u{feff}1\r\n00:00:01,000 --> 00:00:03,500\r\n<i>Hello</i> there\r\nsecond {\\an8}line\r\n\r\n\
                   2\r\n00:00:04,000 --> 00:00:05,000\r\n<font color=\"#fff\">Bye</font>\r\n";
    let cues = parse(content, SubtitleFormat::Srt);
    assert_eq!(cues.len(), 2);
    assert_eq!((cues[0].start, cues[0].end), (1_000_000, 3_500_000));
    assert_eq!(cues[0].text, "Hello there\nsecond line");
    assert_eq!(cues[1].text, "Bye");
}

#[test]
fn vtt_handles_headers_identifiers_settings_and_notes() {
    let content = "WEBVTT - sample\n\nNOTE this block is ignored\n\nintro\n00:01.000 --> 00:02.000 line:0 position:50%\n<v Ada>First</v>\n\n\
                   00:00:03.000 --> 00:00:04.250\nSecond\n";
    let cues = parse(content, SubtitleFormat::Vtt);
    assert_eq!(cues.len(), 2);
    assert_eq!((cues[0].start, cues[0].end), (1_000_000, 2_000_000));
    assert_eq!(cues[0].text, "First");
    assert_eq!(cues[1].end, 4_250_000);
}

#[test]
fn ass_dialogue_honours_the_format_line_and_commas_in_text() {
    let content = "[Script Info]\nTitle: demo\n\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n\n[Events]\n\
                   Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
                   Comment: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,ignored\n\
                   Dialogue: 0,0:00:05.50,0:00:07.00,Default,,0,0,0,,{\\i1}Well, hello{\\i0}\\Nthere\n\
                   Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,First\\hline\n";
    let cues = parse(content, SubtitleFormat::Ass);
    assert_eq!(cues.len(), 2);
    assert_eq!(cues[0].text, "First line");
    assert_eq!((cues[1].start, cues[1].end), (5_500_000, 7_000_000));
    assert_eq!(cues[1].text, "Well, hello\nthere");
}

#[test]
fn malformed_blocks_are_skipped_without_failing_the_file() {
    let content = "1\nnot a timing line\ntext\n\n2\n00:00:02,000 --> 00:00:01,000\nreversed\n\n3\n00:00:05,000 --> 00:00:06,000\nvalid\n";
    let cues = parse(content, SubtitleFormat::Srt);
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].text, "valid");
}

#[test]
fn embedded_events_expose_only_the_dialogue_text() {
    assert_eq!(
        embedded_dialogue_text("12,0,Default,,0,0,0,,{\\an8}Up top, friend\\Nbelow").as_deref(),
        Some("Up top, friend\nbelow")
    );
    assert_eq!(embedded_dialogue_text("12,0,Default"), None);
    assert_eq!(strip_markup("<b>bold</b> {\\pos(1,2)}plain"), "bold plain");
}
