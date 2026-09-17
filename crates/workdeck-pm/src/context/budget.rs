use super::*;

fn measured(packet: &mut ContextPacket) -> Result<usize> {
    // The count itself is serialized. Stabilize its decimal width explicitly.
    loop {
        let size = serde_json::to_vec(packet).map_err(serialization)?.len();
        if size == packet.budget.used_bytes {
            return Ok(size);
        }
        packet.budget.used_bytes = size;
    }
}
pub(super) fn pack(mut packet: ContextPacket) -> Result<ContextPacket> {
    let limit = packet.budget.limit_bytes;
    let entries = packet
        .sections
        .iter_mut()
        .map(|section| {
            section.omitted = section.total;
            if section.total > 0 {
                section.omission_reasons.push("budget".into());
            }
            std::mem::take(&mut section.entries)
        })
        .collect::<Vec<_>>();
    let captured_counts = entries.iter().map(Vec::len).collect::<Vec<_>>();
    packet.budget.omitted_entries = packet.sections.iter().map(|s| s.omitted).sum();
    let mut minimal = packet.clone();
    minimal.budget.limit_bytes = 0;
    loop {
        let size = measured(&mut minimal)?;
        if minimal.budget.minimum_bytes == size && minimal.budget.limit_bytes == size {
            break;
        }
        minimal.budget.minimum_bytes = size;
        minimal.budget.limit_bytes = size;
    }
    let minimum = minimal.budget.minimum_bytes;
    if limit < minimum {
        return Err(PmError::new(ErrorCode::InvalidInput, "context budget cannot contain its required source and omission envelope")
            .details(serde_json::json!({"minimum_required_bytes":minimum,"budget_scope":"compact_context_packet_json"})));
    }
    packet.budget.minimum_bytes = minimum;
    measured(&mut packet)?;
    for (section_index, candidates) in entries.into_iter().enumerate() {
        for entry in candidates {
            // Cheap bound avoids repeatedly serializing a packet for a large omitted document.
            if serde_json::to_vec(&entry).map_err(serialization)?.len()
                > limit
                    .saturating_sub(packet.budget.used_bytes)
                    .saturating_add(32)
            {
                continue;
            }
            packet.sections[section_index].entries.push(entry);
            packet.sections[section_index].omitted -= 1;
            packet.budget.omitted_entries -= 1;
            if measured(&mut packet)? > limit {
                packet.sections[section_index].entries.pop();
                packet.sections[section_index].omitted += 1;
                packet.budget.omitted_entries += 1;
                measured(&mut packet)?;
            }
        }
    }
    for (section, captured) in packet.sections.iter_mut().zip(captured_counts) {
        if section.entries.len() == captured {
            section.omission_reasons.retain(|reason| reason != "budget");
        }
    }
    measured(&mut packet)?;
    debug_assert!(packet.budget.used_bytes <= limit);
    Ok(packet)
}
