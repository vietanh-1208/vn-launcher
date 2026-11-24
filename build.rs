use std::env;
use std::fs;
use std::path::PathBuf;

fn render_dib(size: u32) -> Vec<u8> {
    let (w, h) = (size, size);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut blend = |x: i32, y: i32, c: [u8; 4], alpha: f32| {
        if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h && alpha > 0.0 {
            let a = (alpha.clamp(0.0, 1.0) * (c[3] as f32 / 255.0)).clamp(0.0, 1.0);
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            let dst_a = rgba[i + 3] as f32 / 255.0;
            let out_a = a + dst_a * (1.0 - a);
            if out_a > 0.0 {
                for ch in 0..3 {
                    let src_c = c[ch] as f32 / 255.0;
                    let dst_c = rgba[i + ch] as f32 / 255.0;
                    let out_c = (src_c * a + dst_c * dst_a * (1.0 - a)) / out_a;
                    rgba[i + ch] = (out_c * 255.0).round() as u8;
                }
                rgba[i + 3] = (out_a * 255.0).round() as u8;
            }
        }
    };

    let s = size as f32 / 64.0;
    let r = 15.0 * s;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let cx = fx.clamp(r, w as f32 - r);
            let cy = fy.clamp(r, h as f32 - r);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            if d <= r + 0.8 {
                let cov = (r + 0.5 - d).clamp(0.0, 1.0);
                let t = fy / h as f32;
                let bg = [
                    (16.0 + 14.0 * (1.0 - t)) as u8,
                    (88.0 + 78.0 * (1.0 - t)) as u8,
                    (44.0 + 38.0 * (1.0 - t)) as u8,
                    255,
                ];
                blend(x, y, bg, cov);

                // Pixel-grass top accent
                let col_idx = ((fx / (4.0 * s)) as i32) % 4;
                let step = match col_idx {
                    0 => 2.5,
                    1 => 5.0,
                    2 => 1.5,
                    _ => 4.0,
                };
                let grass_h = (11.5 + step) * s;
                if fy < grass_h && d <= r - 1.5 * s {
                    blend(x, y, [74, 222, 128, 105], cov);
                }

                // Glossy outer emerald rim
                let edge = (r - d).min(fx).min(fy).min(w as f32 - fx).min(h as f32 - fy);
                let rim_w = (3.2 * s).max(1.2);
                if edge < rim_w {
                    let rim = if fy < 24.0 * s {
                        [134, 239, 172, 255]
                    } else {
                        [34, 197, 94, 255]
                    };
                    blend(x, y, rim, cov * (1.0 - (edge / rim_w).clamp(0.0, 1.0) * 0.35));
                }
            }
        }
    }

    let mut stroke = |x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: [u8; 4]| {
        let (x0, y0, x1, y1, t) = (x0 * s, y0 * s, x1 * s, y1 * s, t * s);
        let min_x = (x0.min(x1) - t).floor() as i32;
        let max_x = (x0.max(x1) + t).ceil() as i32;
        let min_y = (y0.min(y1) - t).floor() as i32;
        let max_y = (y0.max(y1) + t).ceil() as i32;
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len2 = (dx * dx + dy * dy).max(0.001);
        let rad = t * 0.5;
        for py in min_y..=max_y {
            for px in min_x..=max_x {
                let fx = px as f32 + 0.5;
                let fy = py as f32 + 0.5;
                let proj = (((fx - x0) * dx + (fy - y0) * dy) / len2).clamp(0.0, 1.0);
                let nx = x0 + proj * dx;
                let ny = y0 + proj * dy;
                let dist = ((fx - nx).powi(2) + (fy - ny).powi(2)).sqrt();
                if dist <= rad + 0.8 {
                    blend(px, py, c, (rad + 0.5 - dist).clamp(0.0, 1.0));
                }
            }
        }
    };

    let shadow = [8, 36, 18, 200];
    let white = [255, 255, 255, 255];
    let mint = [110, 245, 158, 255];
    for (ax, ay, bx, by) in [
        (11.0, 19.0, 19.5, 47.0),
        (19.5, 47.0, 28.0, 19.0),
        (35.0, 47.0, 35.0, 19.0),
        (35.0, 19.0, 51.0, 47.0),
        (51.0, 47.0, 51.0, 19.0),
    ] {
        stroke(ax + 1.6, ay + 2.2, bx + 1.6, by + 2.2, 6.4, shadow);
    }
    stroke(11.0, 19.0, 19.5, 47.0, 6.2, white);
    stroke(19.5, 47.0, 28.0, 19.0, 6.2, white);
    stroke(35.0, 47.0, 35.0, 19.0, 6.2, mint);
    stroke(35.0, 19.0, 51.0, 47.0, 6.0, white);
    stroke(51.0, 47.0, 51.0, 19.0, 6.2, mint);

    // Build 32-bit BGRA BITMAPINFOHEADER + bottom-up pixels + 1-bpp AND mask
    let mask_row_bytes = ((w + 31) / 32) * 4;
    let xor_size = w * h * 4;
    let and_size = mask_row_bytes * h;
    let mut dib = Vec::with_capacity((40 + xor_size + and_size) as usize);
    dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
    dib.extend_from_slice(&(w as i32).to_le_bytes()); // biWidth
    dib.extend_from_slice(&((h * 2) as i32).to_le_bytes()); // biHeight (XOR + AND)
    dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    dib.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression (BI_RGB)
    dib.extend_from_slice(&xor_size.to_le_bytes()); // biSizeImage
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());

    // Bottom-up BGRA pixels
    for y in (0..h).rev() {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            dib.push(rgba[i + 2]); // B
            dib.push(rgba[i + 1]); // G
            dib.push(rgba[i]);     // R
            dib.push(rgba[i + 3]); // A
        }
    }
    // Bottom-up 1-bit AND mask (0 = opaque, 1 = transparent)
    for y in (0..h).rev() {
        for b_idx in 0..mask_row_bytes {
            let mut byte = 0u8;
            for bit in 0..8 {
                let x = b_idx * 8 + bit;
                if x < w {
                    let a = rgba[((y * w + x) * 4 + 3) as usize];
                    if a == 0 {
                        byte |= 1 << (7 - bit);
                    }
                }
            }
            dib.push(byte);
        }
    }
    dib
}

fn build_ico_file(sizes: &[u32], dibs: &[Vec<u8>]) -> Vec<u8> {
    let count = sizes.len() as u16;
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes()); // Type 1 = ICON
    ico.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + (sizes.len() as u32) * 16;
    for (i, &sz) in sizes.iter().enumerate() {
        let dim = if sz >= 256 { 0u8 } else { sz as u8 };
        let len = dibs[i].len() as u32;
        ico.push(dim);
        ico.push(dim);
        ico.push(0);
        ico.push(0);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&len.to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += len;
    }
    for d in dibs {
        ico.extend_from_slice(d);
    }
    ico
}

fn build_grp_icon(sizes: &[u32], dibs: &[Vec<u8>]) -> Vec<u8> {
    let count = sizes.len() as u16;
    let mut grp = Vec::with_capacity(6 + sizes.len() * 14);
    grp.extend_from_slice(&0u16.to_le_bytes());
    grp.extend_from_slice(&1u16.to_le_bytes());
    grp.extend_from_slice(&count.to_le_bytes());
    for (i, &sz) in sizes.iter().enumerate() {
        let dim = if sz >= 256 { 0u8 } else { sz as u8 };
        let len = dibs[i].len() as u32;
        let id = (i as u16) + 1;
        grp.push(dim);
        grp.push(dim);
        grp.push(0);
        grp.push(0);
        grp.extend_from_slice(&1u16.to_le_bytes());
        grp.extend_from_slice(&32u16.to_le_bytes());
        grp.extend_from_slice(&len.to_le_bytes());
        grp.extend_from_slice(&id.to_le_bytes());
    }
    grp
}

/// Builds a minimal x86_64 COFF object file containing a `.rsrc` section with RT_ICON (3) and RT_GROUP_ICON (14).
fn build_coff_rsrc_object(dibs: &[Vec<u8>], grp: &[u8]) -> Vec<u8> {
    let n_icons = dibs.len();
    let total_items = n_icons + 1; // n_icons under RT_ICON (3) + 1 under RT_GROUP_ICON (14)

    let mut rsrc = Vec::new();
    let push_dir = |buf: &mut Vec<u8>, id_count: u16| {
        buf.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
        buf.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
        buf.extend_from_slice(&0u16.to_le_bytes()); // MajorVersion
        buf.extend_from_slice(&0u16.to_le_bytes()); // MinorVersion
        buf.extend_from_slice(&0u16.to_le_bytes()); // NumberOfNamedEntries
        buf.extend_from_slice(&id_count.to_le_bytes()); // NumberOfIdEntries
    };
    let push_entry = |buf: &mut Vec<u8>, id: u32, offset: u32, is_dir: bool| {
        buf.extend_from_slice(&id.to_le_bytes());
        let off = if is_dir { offset | 0x8000_0000 } else { offset };
        buf.extend_from_slice(&off.to_le_bytes());
    };

    // Layout offsets inside `.rsrc`:
    // 0x00: Root Dir (16) + 2 entries (16) = 32 bytes (0..32)
    // 32: Type 3 (RT_ICON) Dir (16) + n_icons * 8 bytes
    let off_type3_dir = 32u32;
    let off_type14_dir = off_type3_dir + 16 + (n_icons as u32) * 8;
    // Type 14 (RT_GROUP_ICON) Dir (16) + 1 * 8 = 24 bytes
    let off_lang_dirs = off_type14_dir + 24;
    // Each item has a Lang Dir (16 + 8 = 24 bytes)
    let off_data_entries = off_lang_dirs + (total_items as u32) * 24;
    // Each item has an IMAGE_RESOURCE_DATA_ENTRY (16 bytes)
    let off_payloads = off_data_entries + (total_items as u32) * 16;

    // 1. Root directory (Types: 3 = RT_ICON, 14 = RT_GROUP_ICON)
    push_dir(&mut rsrc, 2);
    push_entry(&mut rsrc, 3, off_type3_dir, true);
    push_entry(&mut rsrc, 14, off_type14_dir, true);

    // 2. Type 3 (RT_ICON) directory: IDs 1..=n_icons
    push_dir(&mut rsrc, n_icons as u16);
    for i in 0..n_icons {
        let lang_dir_off = off_lang_dirs + (i as u32) * 24;
        push_entry(&mut rsrc, (i as u32) + 1, lang_dir_off, true);
    }

    // 3. Type 14 (RT_GROUP_ICON) directory: ID 1 (MAINICON)
    push_dir(&mut rsrc, 1);
    let grp_lang_dir_off = off_lang_dirs + (n_icons as u32) * 24;
    push_entry(&mut rsrc, 1, grp_lang_dir_off, true);

    // 4. Language directories (Language 0x0409 = en-US)
    for i in 0..total_items {
        push_dir(&mut rsrc, 1);
        let data_entry_off = off_data_entries + (i as u32) * 16;
        push_entry(&mut rsrc, 0x0409, data_entry_off, false);
    }

    // 5. Compute payload offsets (8-byte aligned) and write IMAGE_RESOURCE_DATA_ENTRY structs
    let mut payload_blob = Vec::new();
    let mut reloc_offsets = Vec::with_capacity(total_items);
    for i in 0..total_items {
        while (off_payloads as usize + payload_blob.len()) % 8 != 0 {
            payload_blob.push(0);
        }
        let item_bytes: &[u8] = if i < n_icons { &dibs[i] } else { grp };
        let data_rva_in_rsrc = off_payloads + (payload_blob.len() as u32);
        let reloc_pos_in_rsrc = rsrc.len() as u32; // OffsetToData is the first field of IMAGE_RESOURCE_DATA_ENTRY
        reloc_offsets.push(reloc_pos_in_rsrc);

        rsrc.extend_from_slice(&data_rva_in_rsrc.to_le_bytes()); // OffsetToData (relocated by IMAGE_REL_AMD64_ADDR32NB)
        rsrc.extend_from_slice(&(item_bytes.len() as u32).to_le_bytes()); // Size
        rsrc.extend_from_slice(&0u32.to_le_bytes()); // CodePage
        rsrc.extend_from_slice(&0u32.to_le_bytes()); // Reserved

        payload_blob.extend_from_slice(item_bytes);
    }
    rsrc.extend_from_slice(&payload_blob);

    // Build COFF file:
    // - 20-byte COFF File Header
    // - 40-byte Section Header (`.rsrc`)
    // - `.rsrc` raw data
    // - N * 10-byte COFF Relocations
    // - 1 * 18-byte COFF Symbol (`.rsrc`)
    // - 4-byte String Table (`4u32`)
    let raw_data_ptr = 20u32 + 40u32;
    let reloc_ptr = raw_data_ptr + (rsrc.len() as u32);
    let n_relocs = reloc_offsets.len() as u16;
    let symtab_ptr = reloc_ptr + (n_relocs as u32) * 10;

    let mut coff = Vec::new();
    // COFF File Header
    coff.extend_from_slice(&0x8664u16.to_le_bytes()); // Machine = IMAGE_FILE_MACHINE_AMD64
    coff.extend_from_slice(&1u16.to_le_bytes()); // NumberOfSections = 1
    coff.extend_from_slice(&0u32.to_le_bytes()); // TimeDateStamp
    coff.extend_from_slice(&symtab_ptr.to_le_bytes()); // PointerToSymbolTable
    coff.extend_from_slice(&1u32.to_le_bytes()); // NumberOfSymbols = 1
    coff.extend_from_slice(&0u16.to_le_bytes()); // SizeOfOptionalHeader = 0
    coff.extend_from_slice(&0u16.to_le_bytes()); // Characteristics

    // Section Header (`.rsrc\0\0\0`)
    coff.extend_from_slice(b".rsrc\0\0\0");
    coff.extend_from_slice(&0u32.to_le_bytes()); // VirtualSize
    coff.extend_from_slice(&0u32.to_le_bytes()); // VirtualAddress
    coff.extend_from_slice(&(rsrc.len() as u32).to_le_bytes()); // SizeOfRawData
    coff.extend_from_slice(&raw_data_ptr.to_le_bytes()); // PointerToRawData
    coff.extend_from_slice(&reloc_ptr.to_le_bytes()); // PointerToRelocations
    coff.extend_from_slice(&0u32.to_le_bytes()); // PointerToLinenumbers
    coff.extend_from_slice(&n_relocs.to_le_bytes()); // NumberOfRelocations
    coff.extend_from_slice(&0u16.to_le_bytes()); // NumberOfLinenumbers
    // Characteristics: IMAGE_SCN_CNT_INITIALIZED_DATA (0x40) | IMAGE_SCN_MEM_READ (0x40000000)
    coff.extend_from_slice(&0x4000_0040u32.to_le_bytes());

    // Section data
    coff.extend_from_slice(&rsrc);

    // Relocations (each 10 bytes: VirtualAddress: u32, SymbolTableIndex: u32, Type: u16)
    for off in reloc_offsets {
        coff.extend_from_slice(&off.to_le_bytes());
        coff.extend_from_slice(&0u32.to_le_bytes()); // Symbol 0 (`.rsrc`)
        coff.extend_from_slice(&0x0003u16.to_le_bytes()); // IMAGE_REL_AMD64_ADDR32NB
    }

    // Symbol Table (1 symbol: `.rsrc`, Section 1, StorageClass 3 = IMAGE_SYM_CLASS_STATIC)
    coff.extend_from_slice(b".rsrc\0\0\0");
    coff.extend_from_slice(&0u32.to_le_bytes()); // Value = 0
    coff.extend_from_slice(&1i16.to_le_bytes()); // SectionNumber = 1
    coff.extend_from_slice(&0u16.to_le_bytes()); // Type = 0
    coff.push(3); // StorageClass = IMAGE_SYM_CLASS_STATIC
    coff.push(0); // NumberOfAuxSymbols = 0

    // String Table (empty = 4 bytes length)
    coff.extend_from_slice(&4u32.to_le_bytes());
    coff
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let sizes = [256u32, 64, 48, 32, 16];
    let dibs: Vec<Vec<u8>> = sizes.iter().map(|&s| render_dib(s)).collect();
    let ico = build_ico_file(&sizes, &dibs);
    let grp = build_grp_icon(&sizes, &dibs);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let _ = fs::write("vn-launcher.ico", &ico);
    let coff_path = out_dir.join("vn_icon_rsrc.o");
    fs::write(&coff_path, build_coff_rsrc_object(&dibs, &grp)).expect("write vn_icon_rsrc.o");
    println!("cargo:rustc-link-arg-bins={}", coff_path.display());
}
