pub trait AddFunctions {
    fn add_path(&self, path: &str) -> url::Url;
    fn add_file(&self, file: &str) -> url::Url;
}

impl AddFunctions for url::Url {
    fn add_path(&self, path: &str) -> url::Url {
        let mut path = path.to_string();

        if path.starts_with("/") {
            path.remove(0);
        }

        if !path.ends_with("/") {
            path.push('/')
        }

        let url = self.join(&path);

        match url {
            Ok(url) => url,
            Err(err) => {
                let err_str = format!("Path can't be added: {:?}\n{:?}", path, err);
                tracing::error!("{:?}", &err_str);
                panic!("{:?}", &err_str);
            }
        }
    }

    fn add_file(&self, file: &str) -> url::Url {
        let mut path = file.to_string();

        if path.starts_with("/") {
            path.remove(0);
        }

        let url = self.join(&path);

        match url {
            Ok(url) => url,
            Err(err) => {
                let err_str = format!("File can't be added: {:?}\n{:?}", path, err);
                tracing::error!("{:?}", &err_str);
                panic!("{:?}", &err_str);
            }
        }
    }
}
