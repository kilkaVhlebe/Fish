fn fish_chunk_remove(map: &ChunkMap, key: &(i32, i32)) {
    crate::fish_lib::chunk_remove(map.contains_key(key), *key);
}
