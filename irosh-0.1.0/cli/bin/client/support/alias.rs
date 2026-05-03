use std::str::FromStr;

pub(crate) fn ticket_node_label(ticket: &irosh::Ticket) -> String {
    let node_id = ticket.to_addr().id.to_string();
    shorten_middle(&node_id, 16)
}

pub(crate) fn shorten_ticket(ticket: &irosh::Ticket) -> String {
    shorten_middle(&ticket.to_string(), 40)
}

pub(crate) fn choose_auto_alias(default_alias: &str, ticket_str: &str) -> String {
    sanitize_alias_candidate(default_alias).unwrap_or_else(|| fallback_alias(ticket_str))
}

fn shorten_middle(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_string();
    }

    let keep = (max_len.saturating_sub(3)) / 2;
    let head = &value[..keep];
    let tail_len = max_len.saturating_sub(keep + 3);
    let tail = &value[value.len() - tail_len..];
    format!("{head}...{tail}")
}

fn sanitize_alias_candidate(raw: &str) -> Option<String> {
    const MAX_ALIAS_LEN: usize = 32;

    let mut sanitized = String::with_capacity(raw.len());
    let mut last_was_dash = false;

    for ch in raw.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch);
            last_was_dash = false;
            continue;
        }

        if (ch.is_ascii_whitespace() || ch == '-' || ch == '_' || ch == '.')
            && !last_was_dash
            && !sanitized.is_empty()
        {
            sanitized.push('-');
            last_was_dash = true;
        }
    }

    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        return None;
    }

    let mut alias = sanitized.chars().take(MAX_ALIAS_LEN).collect::<String>();
    alias = alias.trim_matches('-').to_string();
    (!alias.is_empty()).then_some(alias)
}

fn fallback_alias(ticket_str: &str) -> String {
    let ticket = irosh::Ticket::from_str(ticket_str).ok();
    let label = ticket
        .as_ref()
        .map(ticket_node_label)
        .unwrap_or_else(|| "<unknown>".to_string());
    let compact = label.replace('.', "");
    format!("peer-{}", compact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_alias_candidate_normalizes_and_bounds_aliases() {
        assert_eq!(
            sanitize_alias_candidate("Kristency Linux"),
            Some("kristency-linux".to_string())
        );
        assert_eq!(
            sanitize_alias_candidate("  weird___Name...Here  "),
            Some("weird-name-here".to_string())
        );
        assert_eq!(
            sanitize_alias_candidate("A_Very_Long_Alias_Name_With_Many_Sections"),
            Some("a-very-long-alias-name-with-many".to_string())
        );
    }

    #[test]
    fn sanitize_alias_candidate_rejects_empty_results() {
        assert_eq!(sanitize_alias_candidate("...___   ---"), None);
    }

    #[test]
    fn choose_auto_alias_falls_back_to_node_label() {
        let ticket = "endpointabbobyixdbehrhru7mhqcacznay3ploqcve2wclc2bwgtcyh5ov4gaa";
        let alias = choose_auto_alias("!!!", ticket);
        assert!(alias.starts_with("peer-"));
        assert!(alias.len() > 5);
    }
}
