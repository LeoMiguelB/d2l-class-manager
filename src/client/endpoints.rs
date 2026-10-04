use crate::client::D2LClient;
use crate::models::*;
use anyhow::Result;
use futures_util::StreamExt;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

impl D2LClient {
    pub async fn whoami(&self) -> Result<WhoAmIResponse> {
        let url = format!("https://{}/d2l/api/lp/1.47/users/whoami", self.host);
        let res = self.get_resilient(&url).await?;
        let user = res.json::<WhoAmIResponse>().await?;
        Ok(user)
    }

    pub async fn my_enrollments(&self, active_only: bool) -> Result<Vec<MyEnrollment>> {
        let mut items = Vec::new();
        let mut bookmark: Option<String> = None;

        loop {
            let mut url = format!(
                "https://{}/d2l/api/lp/1.47/enrollments/myenrollments/?canAccess=true&sortBy=-StartDate",
                self.host
            );
            if active_only {
                url.push_str("&isActive=true");
            }
            if let Some(ref bm) = bookmark {
                url.push_str(&format!("&bookmark={}", bm));
            }

            let res = self.get_resilient(&url).await?;
            let paged: BookmarkPagedResult<MyEnrollment> = res.json().await?;
            items.extend(paged.items);

            if paged.paging_info.has_more_items && paged.paging_info.bookmark.is_some() {
                bookmark = paged.paging_info.bookmark;
            } else {
                break;
            }
        }
        Ok(items)
    }

    pub async fn news(&self, org_id: i64, since: Option<&str>) -> Result<Vec<NewsItem>> {
        let mut url = format!("https://{}/d2l/api/le/1.80/{}/news/", self.host, org_id);
        if let Some(date) = since {
            url.push_str(&format!("?since={}", date));
        }
        let res = self.get_resilient(&url).await?;
        let items = res.json::<Vec<NewsItem>>().await?;
        Ok(items)
    }

    pub async fn dropbox_folders(&self, org_id: i64) -> Result<Vec<DropboxFolder>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/dropbox/folders/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let folders = res.json::<Vec<DropboxFolder>>().await?;
        Ok(folders)
    }

    pub async fn submissions(&self, org_id: i64, folder_id: i64) -> Result<Vec<Submission>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/{}/dropbox/folders/{}/submissions/mysubmissions/",
            self.host, org_id, folder_id
        );
        let res = self.get_resilient(&url).await?;
        let subs = res.json::<Vec<Submission>>().await?;
        Ok(subs)
    }

    pub async fn content_toc(&self, org_id: i64) -> Result<ContentToc> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/content/toc", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let toc = res.json::<ContentToc>().await?;
        Ok(toc)
    }

    pub async fn forums(&self, org_id: i64) -> Result<Vec<DiscussionForum>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/discussions/forums/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let forums = res.json::<Vec<DiscussionForum>>().await?;
        Ok(forums)
    }

    pub async fn topics(&self, org_id: i64, forum_id: i64) -> Result<Vec<DiscussionTopic>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/{}/discussions/forums/{}/topics/",
            self.host, org_id, forum_id
        );
        let res = self.get_resilient(&url).await?;
        let topics = res.json::<Vec<DiscussionTopic>>().await?;
        Ok(topics)
    }

    pub async fn posts(&self, org_id: i64, forum_id: i64, topic_id: i64) -> Result<Vec<DiscussionPost>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/{}/discussions/forums/{}/topics/{}/posts/?pageNumber=1&pageSize=50",
            self.host, org_id, forum_id, topic_id
        );
        let res = self.get_resilient(&url).await?;
        let posts = res.json::<Vec<DiscussionPost>>().await?;
        Ok(posts)
    }

    pub async fn calendar_events(&self, org_ids_csv: &str) -> Result<Vec<CalendarEvent>> {
        let url = format!(
            "https://{}/d2l/api/le/1.80/calendar/events/myEvents/?orgUnitIdsCSV={}",
            self.host, org_ids_csv
        );
        let res = self.get_resilient(&url).await?;
        let events = res.json::<Vec<CalendarEvent>>().await?;
        Ok(events)
    }

    pub async fn quizzes(&self, org_id: i64) -> Result<Vec<Quiz>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/quizzes/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let quizzes = res.json::<Vec<Quiz>>().await?;
        Ok(quizzes)
    }

    pub async fn grades(&self, org_id: i64) -> Result<Vec<GradeItem>> {
        let url = format!("https://{}/d2l/api/le/1.80/{}/grades/values/myGradeValues/", self.host, org_id);
        let res = self.get_resilient(&url).await?;
        let items = res.json::<Vec<GradeItem>>().await?;
        Ok(items)
    }

    pub async fn download_stream_to_file(&self, url: &str, destination: &Path) -> Result<String> {
        let res = self.get_resilient(url).await?;

        let filename = res
            .headers()
            .get("content-disposition")
            .and_then(|cd| cd.to_str().ok())
            .and_then(|cd_str| {
                let re = regex::Regex::new(r#"filename[*]?=(?:UTF-8'')?"?([^";]+)"?"#).ok()?;
                re.captures(cd_str).and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            })
            .or_else(|| {
                url.split('?')
                    .next()
                    .and_then(|u| u.rsplit('/').next())
                    .map(|s| s.replace("%20", " "))
            })
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "downloaded_content.bin".to_string());

        let target_file = if destination.is_dir() {
            destination.join(&filename)
        } else {
            destination.to_path_buf()
        };

        if let Some(parent) = target_file.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut file = File::create(&target_file).await?;
        let mut stream = res.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(target_file.display().to_string())
    }
}
