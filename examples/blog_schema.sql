-- Esquema de prueba: blog con usuarios + anexos.
-- Sin tablas de permisos ni roles. MySQL 8+ / MariaDB 10.5+.
-- Base para `tests/runtime_mysql.rs` y `examples/runtime_mysql_blog.rs`.

CREATE DATABASE IF NOT EXISTS `example_medoo_db`
  DEFAULT CHARSET utf8mb4 COLLATE utf8mb4_unicode_ci;

USE `example_medoo_db`;

DROP TABLE IF EXISTS `attachments`;
DROP TABLE IF EXISTS `comments`;
DROP TABLE IF EXISTS `post_categories`;
DROP TABLE IF EXISTS `posts`;
DROP TABLE IF EXISTS `categories`;
DROP TABLE IF EXISTS `users`;

CREATE TABLE `users` (
  `id`         BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  `email`      VARCHAR(255) NOT NULL UNIQUE,
  `name`       VARCHAR(120) NOT NULL,
  `bio`        TEXT,
  `meta`       JSON,
  `active`     TINYINT(1)   NOT NULL DEFAULT 1,
  `created_at` TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `categories` (
  `id`   BIGINT      NOT NULL AUTO_INCREMENT PRIMARY KEY,
  `name` VARCHAR(80) NOT NULL UNIQUE,
  `slug` VARCHAR(80) NOT NULL UNIQUE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `posts` (
  `id`         BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  `user_id`    BIGINT       NOT NULL,
  `title`      VARCHAR(200) NOT NULL,
  `slug`       VARCHAR(220) NOT NULL UNIQUE,
  `body`       MEDIUMTEXT   NOT NULL,
  `tags`       JSON,
  `views`      INT          NOT NULL DEFAULT 0,
  `published`  TINYINT(1)   NOT NULL DEFAULT 0,
  `created_at` TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `updated_at` TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  CONSTRAINT `fk_posts_user` FOREIGN KEY (`user_id`) REFERENCES `users`(`id`) ON DELETE CASCADE,
  KEY `idx_posts_user`      (`user_id`),
  KEY `idx_posts_published` (`published`, `created_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `post_categories` (
  `post_id`     BIGINT NOT NULL,
  `category_id` BIGINT NOT NULL,
  PRIMARY KEY (`post_id`, `category_id`),
  CONSTRAINT `fk_pc_post` FOREIGN KEY (`post_id`)     REFERENCES `posts`(`id`)      ON DELETE CASCADE,
  CONSTRAINT `fk_pc_cat`  FOREIGN KEY (`category_id`) REFERENCES `categories`(`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `comments` (
  `id`         BIGINT    NOT NULL AUTO_INCREMENT PRIMARY KEY,
  `post_id`    BIGINT    NOT NULL,
  `user_id`    BIGINT    NOT NULL,
  `body`       TEXT      NOT NULL,
  `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  CONSTRAINT `fk_c_post` FOREIGN KEY (`post_id`) REFERENCES `posts`(`id`) ON DELETE CASCADE,
  CONSTRAINT `fk_c_user` FOREIGN KEY (`user_id`) REFERENCES `users`(`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `attachments` (
  `id`          BIGINT       NOT NULL AUTO_INCREMENT PRIMARY KEY,
  `post_id`     BIGINT       NOT NULL,
  `filename`    VARCHAR(255) NOT NULL,
  `mime`        VARCHAR(100) NOT NULL,
  `size_bytes`  BIGINT       NOT NULL,
  `uploaded_at` TIMESTAMP    NOT NULL DEFAULT CURRENT_TIMESTAMP,
  CONSTRAINT `fk_a_post` FOREIGN KEY (`post_id`) REFERENCES `posts`(`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
