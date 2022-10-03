use askama::Template;

#[derive(Template)]
#[template(path = "tar_index.html")]
pub struct TarIndex {
    pub valid_until: chrono::NaiveDateTime,
    pub craeted_at: chrono::NaiveDateTime,
    pub files: Vec<TarFileInfo>,
    pub id : String,
    pub hostname : String,
}

pub struct TarFileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub offset: u64,
    pub is_dir: bool,
    pub m_time: chrono::NaiveDateTime,
}
