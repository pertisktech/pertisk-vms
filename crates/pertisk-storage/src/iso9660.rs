//! Minimal ISO 9660 + Joliet image for cloud-init NoCloud (`cidata` volume id).

const SECTOR: usize = 2048;

pub fn cidata_iso(user_data: &[u8], meta_data: &[u8]) -> Vec<u8> {
    write_joliet_iso(
        "cidata",
        &[("user-data", user_data), ("meta-data", meta_data)],
    )
}

fn write_joliet_iso(volume_id: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let lba_pvd = 16u32;
    let lba_svd = 17;
    let lba_term = 18;
    let lba_pt_le = 19;
    let lba_pt_be = 20;
    let lba_jpt_le = 21;
    let lba_jpt_be = 22;
    let lba_root_iso = 23;
    let lba_root_joliet = 24;
    let mut next = 25u32;
    let mut extents = Vec::new();
    for (_, data) in files {
        let sectors = data.len().div_ceil(SECTOR) as u32;
        extents.push((next, sectors.max(1)));
        next += sectors.max(1);
    }
    let volume_sectors = next;

    let mut image = vec![0u8; volume_sectors as usize * SECTOR];

    write_pvd(
        &mut image[lba_pvd as usize * SECTOR..][..SECTOR],
        volume_id,
        volume_sectors,
        lba_root_iso,
        lba_pt_le,
        lba_pt_be,
    );
    write_svd(
        &mut image[lba_svd as usize * SECTOR..][..SECTOR],
        volume_id,
        volume_sectors,
        lba_root_joliet,
        lba_jpt_le,
        lba_jpt_be,
    );
    image[lba_term as usize * SECTOR] = 255;
    image[lba_term as usize * SECTOR + 1..lba_term as usize * SECTOR + 6].copy_from_slice(b"CD001");
    image[lba_term as usize * SECTOR + 6] = 1;

    write_path_table(
        &mut image[lba_pt_le as usize * SECTOR..][..SECTOR],
        lba_root_iso,
        false,
    );
    write_path_table(
        &mut image[lba_pt_be as usize * SECTOR..][..SECTOR],
        lba_root_iso,
        true,
    );
    write_path_table(
        &mut image[lba_jpt_le as usize * SECTOR..][..SECTOR],
        lba_root_joliet,
        false,
    );
    write_path_table(
        &mut image[lba_jpt_be as usize * SECTOR..][..SECTOR],
        lba_root_joliet,
        true,
    );

    let iso_root = iso_root_bytes(lba_root_iso, files, &extents);
    let joliet_root = joliet_root_bytes(lba_root_joliet, files, &extents);
    image[lba_root_iso as usize * SECTOR..][..iso_root.len()].copy_from_slice(&iso_root);
    image[lba_root_joliet as usize * SECTOR..][..joliet_root.len()].copy_from_slice(&joliet_root);

    for (i, (_, data)) in files.iter().enumerate() {
        let (lba, _) = extents[i];
        let dest = lba as usize * SECTOR;
        image[dest..dest + data.len()].copy_from_slice(data);
    }
    image
}

fn write_pvd(
    sector: &mut [u8],
    volume_id: &str,
    volume_sectors: u32,
    root_lba: u32,
    pt_le: u32,
    pt_be: u32,
) {
    sector[0] = 1;
    sector[1..6].copy_from_slice(b"CD001");
    sector[6] = 1;
    pad_str(&mut sector[40..72], &volume_id.to_ascii_uppercase());
    sector[80..88].copy_from_slice(&both32(volume_sectors));
    sector[120..124].copy_from_slice(&both16(SECTOR as u16));
    let pt_size = 10u32;
    sector[124..132].copy_from_slice(&both32(pt_size));
    sector[132..136].copy_from_slice(&pt_le.to_le_bytes());
    sector[140..144].copy_from_slice(&pt_be.to_be_bytes());
    write_dir_record_into(
        &mut sector[156..190],
        root_lba,
        SECTOR as u32,
        0x02,
        &[0],
        false,
    );
    sector[881] = 1; // file structure version
}

fn write_svd(
    sector: &mut [u8],
    volume_id: &str,
    volume_sectors: u32,
    root_lba: u32,
    pt_le: u32,
    pt_be: u32,
) {
    sector[0] = 2;
    sector[1..6].copy_from_slice(b"CD001");
    sector[6] = 1;
    ucs2_pad(&mut sector[40..72], volume_id);
    sector[80..88].copy_from_slice(&both32(volume_sectors));
    // Joliet UCS-2 level 3 escape
    sector[88] = 0x25;
    sector[89] = 0x2f;
    sector[90] = 0x45;
    sector[120..124].copy_from_slice(&both16(SECTOR as u16));
    let pt_size = 10u32;
    sector[124..132].copy_from_slice(&both32(pt_size));
    sector[132..136].copy_from_slice(&pt_le.to_le_bytes());
    sector[140..144].copy_from_slice(&pt_be.to_be_bytes());
    write_dir_record_into(
        &mut sector[156..190],
        root_lba,
        SECTOR as u32,
        0x02,
        &[0],
        true,
    );
    sector[881] = 1;
}

fn write_path_table(sector: &mut [u8], dir_lba: u32, be: bool) {
    sector[0] = 1;
    sector[1] = 0;
    if be {
        sector[2..6].copy_from_slice(&dir_lba.to_be_bytes());
        sector[6..8].copy_from_slice(&1u16.to_be_bytes());
    } else {
        sector[2..6].copy_from_slice(&dir_lba.to_le_bytes());
        sector[6..8].copy_from_slice(&1u16.to_le_bytes());
    }
    sector[8] = 0;
}

fn iso_root_bytes(root_lba: u32, files: &[(&str, &[u8])], extents: &[(u32, u32)]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(dir_record(root_lba, SECTOR as u32, 0x02, &[0], false));
    buf.extend(dir_record(root_lba, SECTOR as u32, 0x02, &[1], false));
    for (i, (name, data)) in files.iter().enumerate() {
        let (lba, _) = extents[i];
        let ident = iso_ident(name);
        buf.extend(dir_record(lba, data.len() as u32, 0, &ident, false));
    }
    buf
}

fn joliet_root_bytes(root_lba: u32, files: &[(&str, &[u8])], extents: &[(u32, u32)]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(dir_record(root_lba, SECTOR as u32, 0x02, &[0], true));
    buf.extend(dir_record(root_lba, SECTOR as u32, 0x02, &[1], true));
    for (i, (name, data)) in files.iter().enumerate() {
        let (lba, _) = extents[i];
        buf.extend(dir_record(lba, data.len() as u32, 0, &ucs2(name), true));
    }
    buf
}

fn iso_ident(name: &str) -> Vec<u8> {
    let stem: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let mut ident = stem.into_bytes();
    ident.extend_from_slice(b".;1");
    ident
}

fn dir_record(extent: u32, data_len: u32, flags: u8, name: &[u8], _joliet: bool) -> Vec<u8> {
    let mut rec = vec![0u8; 33 + name.len()];
    rec[0] = rec.len() as u8;
    rec[2..10].copy_from_slice(&both32(extent));
    rec[10..18].copy_from_slice(&both32(data_len));
    rec[18] = 124; // 2024-1900
    rec[19] = 1;
    rec[20] = 1;
    rec[25] = flags;
    rec[32] = name.len() as u8;
    rec[33..].copy_from_slice(name);
    if rec.len() % 2 == 1 {
        rec.push(0);
        rec[0] = rec.len() as u8;
    }
    rec
}

fn write_dir_record_into(
    dest: &mut [u8],
    extent: u32,
    data_len: u32,
    flags: u8,
    name: &[u8],
    joliet: bool,
) {
    let rec = dir_record(extent, data_len, flags, name, joliet);
    dest[..rec.len()].copy_from_slice(&rec);
}

fn both16(n: u16) -> [u8; 4] {
    let le = n.to_le_bytes();
    let be = n.to_be_bytes();
    [le[0], le[1], be[0], be[1]]
}

fn both32(n: u32) -> [u8; 8] {
    let le = n.to_le_bytes();
    let be = n.to_be_bytes();
    [le[0], le[1], le[2], le[3], be[0], be[1], be[2], be[3]]
}

fn pad_str(dest: &mut [u8], s: &str) {
    dest.fill(b' ');
    let bytes = s.as_bytes();
    let n = bytes.len().min(dest.len());
    dest[..n].copy_from_slice(&bytes[..n]);
}

fn ucs2(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

fn ucs2_pad(dest: &mut [u8], s: &str) {
    dest.fill(0);
    for (i, ch) in dest.chunks_exact_mut(2).enumerate() {
        let unit = s.encode_utf16().nth(i).unwrap_or(0x0020);
        ch.copy_from_slice(&unit.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidata_volume_and_joliet_names() {
        let iso = cidata_iso(b"#cloud-config\n", b"instance-id: i-1\n");
        assert_eq!(&iso[32768..32774], b"\x01CD001");
        let vol = std::str::from_utf8(&iso[32768 + 40..32768 + 46]).unwrap();
        assert_eq!(vol, "CIDATA");
        assert_eq!(&iso[34816..34822], b"\x02CD001");
        let joliet_user: Vec<u8> = "user-data"
            .encode_utf16()
            .flat_map(|u| u.to_be_bytes())
            .collect();
        assert!(
            iso.windows(joliet_user.len()).any(|w| w == joliet_user),
            "missing Joliet user-data name"
        );
        let user = b"#cloud-config\n";
        let meta = b"instance-id: i-1\n";
        assert!(iso.windows(user.len()).any(|w| w == user));
        assert!(iso.windows(meta.len()).any(|w| w == meta));
    }
}
