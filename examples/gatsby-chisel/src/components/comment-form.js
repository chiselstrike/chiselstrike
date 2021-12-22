import React, { useState, useCallback } from "react"

import api from "../services/api"
import { getCommentsFromChisel } from "./comment-section"

export default function CommentForm({ postId, setIsLoading, setComments }) {
  const [newComment, setNewComment] = useState("")

  const handleCommentCreation = useCallback(async () => {
    setIsLoading(true)
    try {
      await api.post("comments", {
        content: newComment,
        postId: postId,
      })

      setComments(await getCommentsFromChisel(postId))
    } catch (error) {
      console.error(error)
    }

    setNewComment("")
    setIsLoading(false)
  }, [postId, setIsLoading, setComments, newComment])

  const handleResetComment = useCallback(() => {
    setNewComment("")
  }, [])

  const handleChangeNewComment = useCallback(event => {
    setNewComment(event.target.value)
  }, [])

  return (
    <section>
      <h3 className="font-medium mb-1 text-base"> Leave a Comment</h3>
      <div className="flex justify-between items-center">
        <small className=" tracking-wide font-light">
          {" "}
          Type your Comment Below
        </small>
      </div>
      <div className="mt-4 border border-grey w-full border-1 rounded p-2 relative focus:border-red">
        <input
          type="text"
          onChange={handleChangeNewComment}
          value={newComment}
          className="pl-8 text-grey-dark font-light w-full text-sm  tracking-wide"
          placeholder="Type your comment..."
        />
      </div>
      <div className="modal__footer mt-6">
        <div className="text-right">
          <button onClick={handleResetComment} className="common-button">
            Cancel
          </button>
          <button onClick={handleCommentCreation} className="common-button">
            Submit Comment
          </button>
        </div>
      </div>
    </section>
  )
}
