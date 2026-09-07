@file:OptIn(org.readium.r2.shared.ExperimentalReadiumApi::class)

package com.mewo.reader.data

import org.readium.r2.shared.publication.Publication
import org.readium.r2.shared.publication.services.content.Content
import org.readium.r2.shared.publication.services.content.content

/**
 * Turns a Readium publication into timeline posts. One paragraph stays one post.
 */
class FeedExtractor {
    suspend fun extract(publication: Publication): List<FeedPost> {
        val content = publication.content() ?: return emptyList()
        val posts = mutableListOf<FeedPost>()
        var chapter = "Start"
        var index = 0

        for (element in content.elements()) {
            val textElement = element as? Content.TextElement ?: continue
            val text = textElement.text.trim()
            if (text.isEmpty()) continue

            val kind = when (textElement.role) {
                is Content.TextElement.Role.Heading -> {
                    chapter = text
                    PostKind.Heading
                }
                else -> PostKind.Body
            }

            posts += FeedPost(
                id = index.toString(),
                kind = kind,
                text = text,
                chapter = chapter,
            )
            index += 1
        }
        return posts
    }
}
