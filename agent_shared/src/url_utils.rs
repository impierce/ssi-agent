pub trait UrlAddFunctions {
    fn add_path(&self, path: &str) -> url::Url;
    fn add_file(&self, file: &str) -> url::Url;
}

fn create_trailing_slash_url(url: &url::Url) -> url::Url {
         if !url.path().ends_with('/') {
             let res = url::Url::parse(&format!("{}/", url)).unwrap();
             tracing::info!("res: {:?}", res);
             res
        } else {
            url.clone()
        }
}

impl UrlAddFunctions for url::Url {
    fn add_path(&self, path: &str) -> url::Url {
        let mut path = path.to_string();

        if path.starts_with('/') {
            path.remove(0);
        }

        if !path.ends_with('/') {
            path.push('/')
        }

        let url = create_trailing_slash_url(self).join(&path);

        println!("{:?}", url);

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

        if path.starts_with('/')  {
            path.remove(0);
        }

        let url = create_trailing_slash_url(self).join(&path);

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

#[cfg(test)]
mod tests {
    use url::Url;
    use crate::url_utils::UrlAddFunctions;

    #[test]
    fn test_add_path() {
        let url = Url::parse("https://test.example.com/unicore/").unwrap();
        let res: String = url.add_path("/some-path/").into();

        assert_eq!("https://test.example.com/unicore/some-path/", &res);

        let res: String = url.add_path("some-path/").into();

        assert_eq!("https://test.example.com/unicore/some-path/", &res);

        let res: String = url.add_path("some-path").into();

        assert_eq!("https://test.example.com/unicore/some-path/", &res);

        // With base path (no trailing slash)
        let url = Url::parse("https://test.example.com/unicore").unwrap();
        let res: String = url.add_path("/some-path/").into();

        assert_eq!("https://test.example.com/unicore/some-path/", &res);

        let url = Url::parse("https://test.example.com").unwrap();
        let res: String = url.add_path("/some-path/").into();

        assert_eq!("https://test.example.com/some-path/", &res);
    }

    #[test]
    fn test_add_file() {
        let url = Url::parse("https://test.example.com/unicore/").unwrap();
        let res: String = url.add_file("/some-file.txt").into();

        assert_eq!("https://test.example.com/unicore/some-file.txt", &res);

        let res: String = url.add_file("some-file.txt").into();

        assert_eq!("https://test.example.com/unicore/some-file.txt", &res);

        let url = Url::parse("https://test.example.com/unicore").unwrap();
        let res: String = url.add_file("/some-file.txt").into();

        assert_eq!("https://test.example.com/unicore/some-file.txt", &res);

        let url = Url::parse("https://test.example.com").unwrap();
        let res: String = url.add_file("/some-file.txt").into();

        assert_eq!("https://test.example.com/some-file.txt", &res);

        let url = Url::parse("https://test.example.com/").unwrap();
        let res: String = url.add_file("/some-file.txt").into();

        assert_eq!("https://test.example.com/some-file.txt", &res);
    }
}
