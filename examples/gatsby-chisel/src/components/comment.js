import React from "react"
import { formatDistance } from "date-fns"

const now = new Date()

const Comment = ({ id, content, postedAt }) => {
  return (
    <div key={id} className="flex flex-col mb-2">
      <div className="flex justify-between">
        <p className="text-grey-dark font-light italic">{`Anonymous said:`}</p>
        <p className="text-grey-dark font-light italic">
          {`${formatDistance(new Date(postedAt), now)} ago`}
        </p>
      </div>
      <p className="text-grey-dark">{content}</p>
    </div>
  )
}

export default Comment
