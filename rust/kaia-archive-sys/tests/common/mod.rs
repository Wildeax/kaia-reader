//! Test-only RAR 4 stored records. No compressor or proprietary RAR tool is used.
//! CRCs cover the header after its CRC field and the uncompressed file bytes.

pub fn rar(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut result = b"Rar!\x1a\x07\x00".to_vec();
    result.extend(header(0x73, 0, &[0; 6]));
    for (name, data, declared) in entries {
        let mut fields = Vec::new();
        fields.extend((data.len() as u32).to_le_bytes());
        fields.extend(declared.to_le_bytes());
        fields.push(3); // Unix host.
        fields.extend(crc32fast::hash(data).to_le_bytes());
        fields.extend(0u32.to_le_bytes()); // DOS timestamp, unused by these tests.
        fields.extend([20, 0x30]); // RAR 2.0 extraction version, stored method.
        fields.extend((name.len() as u16).to_le_bytes());
        fields.extend(0o100644u32.to_le_bytes());
        fields.extend(name.as_bytes());
        result.extend(header(0x74, 0x8000, &fields));
        result.extend(*data);
    }
    result.extend(header(0x7b, 0, &[]));
    result
}

fn header(kind: u8, flags: u16, fields: &[u8]) -> Vec<u8> {
    let mut header = vec![kind];
    header.extend(flags.to_le_bytes());
    header.extend(((7 + fields.len()) as u16).to_le_bytes());
    header.extend(fields);
    let mut result = (crc32fast::hash(&header) as u16).to_le_bytes().to_vec();
    result.extend(header);
    result
}
