//! Read audio/subtitle track metadata from mpv (`track-list/*` properties).

use libmpv2::Mpv;

use super::state::TrackInfo;

fn mpv_prop_i64(mpv: &Mpv, name: &str) -> Option<i64> {
    mpv.get_property::<i64>(name)
        .ok()
        .or_else(|| mpv.get_property::<String>(name).ok()?.parse().ok())
}

fn mpv_prop_string(mpv: &Mpv, name: &str) -> Option<String> {
    if let Ok(value) = mpv.get_property::<String>(name) {
        if value.is_empty() || value == "no" {
            return None;
        }
        return Some(value);
    }
    None
}

fn track_list_count(mpv: &Mpv) -> i64 {
    mpv_prop_i64(mpv, "track-list/count").unwrap_or(0).max(0)
}

fn track_type_at(mpv: &Mpv, index: i64) -> Option<String> {
    mpv_prop_string(mpv, &format!("track-list/{index}/type"))
}

pub fn audio_track_count(mpv: &Mpv) -> u32 {
    count_tracks(mpv, "audio")
}

pub fn subtitle_track_count(mpv: &Mpv) -> u32 {
    count_tracks(mpv, "sub")
}

fn count_tracks(mpv: &Mpv, kind: &str) -> u32 {
    let count = track_list_count(mpv);
    let mut n = 0u32;
    for index in 0..count {
        if track_type_at(mpv, index).as_deref() == Some(kind) {
            n += 1;
        }
    }
    n
}

fn track_info_at(mpv: &Mpv, index: i64) -> Option<TrackInfo> {
    let id = mpv_prop_i64(mpv, &format!("track-list/{index}/id"))? as u32;
    let title = mpv_prop_string(mpv, &format!("track-list/{index}/title"));
    let language = mpv_prop_string(mpv, &format!("track-list/{index}/lang"));
    Some(TrackInfo {
        index: id,
        label: format_track_label(title.as_deref(), language.as_deref(), "Track"),
        language,
    })
}

pub fn format_track_label(title: Option<&str>, language: Option<&str>, fallback: &str) -> String {
    if let Some(title) = title.filter(|s| !s.is_empty()) {
        return title.to_string();
    }
    if let Some(lang) = language.filter(|s| !s.is_empty()) {
        return format!("{fallback} ({lang})");
    }
    fallback.to_string()
}

pub fn refresh_playback_tracks(mpv: &Mpv, audio_tracks: &mut Vec<TrackInfo>, subtitle_tracks: &mut Vec<TrackInfo>) {
    audio_tracks.clear();
    subtitle_tracks.clear();

    let count = track_list_count(mpv);
    for index in 0..count {
        let Some(kind) = track_type_at(mpv, index) else {
            continue;
        };
        let Some(info) = track_info_at(mpv, index) else {
            continue;
        };
        match kind.as_str() {
            "audio" => audio_tracks.push(info),
            "sub" => subtitle_tracks.push(info),
            _ => {}
        }
    }
}

pub fn current_aid(mpv: &Mpv) -> Option<i64> {
    mpv_prop_i64(mpv, "aid")
}

pub fn current_sid(mpv: &Mpv) -> Option<i64> {
    mpv_prop_i64(mpv, "sid")
}

pub fn label_for_track_id(mpv: &Mpv, id: i64, kind: &str, fallback: &str) -> String {
    let count = track_list_count(mpv);
    for index in 0..count {
        if track_type_at(mpv, index).as_deref() != Some(kind) {
            continue;
        }
        if mpv_prop_i64(mpv, &format!("track-list/{index}/id")) == Some(id) {
            let title = mpv_prop_string(mpv, &format!("track-list/{index}/title"));
            let language = mpv_prop_string(mpv, &format!("track-list/{index}/lang"));
            return format_track_label(title.as_deref(), language.as_deref(), fallback);
        }
    }
    format!("{fallback} #{id}")
}

pub fn current_audio_label(mpv: &Mpv) -> String {
    current_aid(mpv)
        .map(|id| label_for_track_id(mpv, id, "audio", "Audio"))
        .unwrap_or_else(|| "Audio".to_string())
}

pub fn current_subtitle_label(mpv: &Mpv) -> String {
    current_sid(mpv)
        .map(|id| label_for_track_id(mpv, id, "sub", "Subtitle"))
        .unwrap_or_else(|| "Subtitles off".to_string())
}

#[cfg(test)]
mod tests {
    use super::format_track_label;

    #[test]
    fn label_prefers_title() {
        assert_eq!(
            format_track_label(Some("Commentary"), Some("eng"), "Audio"),
            "Commentary"
        );
    }

    #[test]
    fn label_falls_back_to_language() {
        assert_eq!(
            format_track_label(None, Some("fra"), "Subtitle"),
            "Subtitle (fra)"
        );
    }
}
