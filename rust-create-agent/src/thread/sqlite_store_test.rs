    async fn make_store() -> (SqliteThreadStore, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let store = SqliteThreadStore::new(dir.path().join("test.db"))
            .await
            .unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn test_create_append_load() {
        let (store, _dir) = make_store().await;
        let meta = ThreadMeta::new("/tmp");
        let id = store.create_thread(meta).await.unwrap();

        let msgs = vec![BaseMessage::human("Hello"), BaseMessage::ai("Hi there")];
        store.append_messages(&id, &msgs).await.unwrap();

        let loaded = store.load_messages(&id).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].content(), "Hello");
        assert_eq!(loaded[1].content(), "Hi there");
    }

    #[tokio::test]
    async fn test_list_threads_order() {
        let (store, _dir) = make_store().await;

        let m1 = ThreadMeta::new("/a");
        let id1 = store.create_thread(m1).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let m2 = ThreadMeta::new("/b");
        let id2 = store.create_thread(m2).await.unwrap();

        // 给 id2 追加消息，更新 updated_at
        store
            .append_messages(&id2, &[BaseMessage::human("msg")])
            .await
            .unwrap();

        let list = store.list_threads().await.unwrap();
        assert_eq!(list.len(), 2);
        // id2 updated_at 更新，应排在第一位
        assert_eq!(list[0].id, id2);
        assert_eq!(list[1].id, id1);
    }

    #[tokio::test]
    async fn test_delete_thread_cascade() {
        let (store, _dir) = make_store().await;
        let meta = ThreadMeta::new("/tmp");
        let id = store.create_thread(meta).await.unwrap();
        store
            .append_messages(&id, &[BaseMessage::human("msg")])
            .await
            .unwrap();

        store.delete_thread(&id).await.unwrap();

        // 消息应该被级联删除
        let msgs = store.load_messages(&id).await;
        // 线程不存在时 load_messages 应返回空（因为 SELECT 无结果）
        assert!(msgs.unwrap().is_empty());

        // 元数据应不存在
        let meta_result = store.load_meta(&id).await;
        assert!(meta_result.is_err());
    }

    #[tokio::test]
    async fn test_message_order_after_multiple_appends() {
        let (store, _dir) = make_store().await;
        let meta = ThreadMeta::new("/tmp");
        let id = store.create_thread(meta).await.unwrap();

        store
            .append_messages(&id, &[BaseMessage::human("msg1")])
            .await
            .unwrap();
        store
            .append_messages(&id, &[BaseMessage::ai("reply1")])
            .await
            .unwrap();
        store
            .append_messages(&id, &[BaseMessage::human("msg2")])
            .await
            .unwrap();

        let loaded = store.load_messages(&id).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].content(), "msg1");
        assert_eq!(loaded[1].content(), "reply1");
        assert_eq!(loaded[2].content(), "msg2");
    }

    #[tokio::test]
    async fn test_title_auto_set() {
        let (store, _dir) = make_store().await;
        let meta = ThreadMeta::new("/tmp");
        let id = store.create_thread(meta).await.unwrap();

        store
            .append_messages(&id, &[BaseMessage::human("这是一条测试消息")])
            .await
            .unwrap();

        let loaded_meta = store.load_meta(&id).await.unwrap();
        assert!(loaded_meta.title.is_some());
        assert!(loaded_meta.title.unwrap().contains("这是一条测试消息"));
    }

    #[tokio::test]
    async fn test_update_title() {
        let (store, _dir) = make_store().await;
        let meta = ThreadMeta::new("/tmp");
        let id = store.create_thread(meta).await.unwrap();

        store.update_title(&id, "new title").await.unwrap();
        let loaded = store.load_meta(&id).await.unwrap();
        assert_eq!(loaded.title.as_deref(), Some("new title"));
    }

    #[tokio::test]
    async fn test_update_title_updates_timestamp() {
        let (store, _dir) = make_store().await;
        let meta = ThreadMeta::new("/tmp");
        let id = store.create_thread(meta).await.unwrap();

        let before = store.load_meta(&id).await.unwrap().updated_at;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        store.update_title(&id, "updated").await.unwrap();
        let after = store.load_meta(&id).await.unwrap().updated_at;
        assert!(
            after > before,
            "updated_at should be newer after update_title"
        );
    }
