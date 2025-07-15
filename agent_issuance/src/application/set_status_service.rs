
/// Sets the status at the specified index to the given value.
/// Always enlarges the status list to the required size if it is not already large enough.
/// Therefore an index can never be out of bounds, also stores the index for the issuer to know what index is used.
pub fn set_index(&mut self, index: usize, value: u8) -> Result<(), OAuthTSLError> {
    self.status_list.set_index(index, value)?;
    if !self.used_indices.contains(&index) {
        self.used_indices.push(index);
    }

    Ok(())
}

/// Sets the status at a random index to the given value.
/// Always enlarges the status list to the required size if it is not already large enough.
pub fn set_random_index(&mut self, value: u8) -> Result<(), OAuthTSLError> {
    let mut size = self.status_list.status_list.len() * (8 / self.status_list.status_size.as_usize());

    if self.used_indices.len() >= size {
        self.status_list.status_list.resize(size + 20, 0);
        size = self.status_list.status_list.len() * (8 / self.status_list.status_size.as_usize());
    }

    let mut rng = rand::rng();
    let mut index;

    loop {
        index = rng.random_range(0..size);
        if !self.used_indices.contains(&index) {
            break;
        }
    }

    self.status_list.set_index(index, value)?;
    if !self.used_indices.contains(&index) {
        self.used_indices.push(index);
    }

    Ok(())
}
