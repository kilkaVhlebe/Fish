fn fish_chunk_insert(map: &ChunkMap, key: (i32, i32)) {
    crate::fish_lib::chunk_insert(map.contains_key(&key), key);
}
