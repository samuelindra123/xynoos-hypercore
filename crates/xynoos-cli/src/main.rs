fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut store = xynoos_hypermemory::PageStore::with_capacity(1)?;
    let page = vec![0; 4096];
    let id = store.insert(&page)?;
    assert_eq!(store.get(id)?, page);
    println!("Halaman {:?}: {:?}", id, store.stats());
    Ok(())
}
