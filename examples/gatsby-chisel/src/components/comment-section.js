import React, { useState, useEffect } from "react"

import Comment from "./comment"
import CommentForm from "./comment-form"
import Loader from "./loader"

import api from "../services/api"

export const getCommentsFromChisel = async postId => {
  try {
    const res = await api.get("comments", {
      params: {
        postId,
      },
    })
    return res.data
  } catch (error) {
    console.error(error)
  }
}

export default function CommentSection({ postId }) {
  const [comments, setComments] = useState([])
  const [isLoading, setIsLoading] = useState(false)

  useEffect(() => {
    const getComments = async () => {
      setComments(await getCommentsFromChisel(postId))
    }
    getComments()
  }, [postId])

  return (
    <section className="relative border-2 border-slate-100 rounded-md px-4 py-5 mb-4 mt-6">
      {isLoading && <Loader />}
      <CommentForm
        postId={postId}
        setIsLoading={setIsLoading}
        setComments={setComments}
      />
      <section className="mt-6">
        <h3 className="text-grey-darkest font-medium mb-1 text-base">
          Comments:
        </h3>
        {comments?.map(comm => (
          <Comment
            key={comm.id}
            id={comm.id}
            rating={comm.rating}
            content={comm.content}
            postedAt={comm.postedAt}
          />
        ))}
      </section>
    </section>
  )
}
