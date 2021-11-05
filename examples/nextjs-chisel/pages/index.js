import Head from 'next/head'
import Image from 'next/image'
import styles from '../styles/Home.module.css'
import React, {useEffect} from "react";

export default function Home() {
  const [peopleData, setPeopleData] = React.useState([])

  function fetch_people() {
    fetch('/api/get_all_people', {
      method: 'GET',
    }).then((res) => {
      res.json().then((jsonData) => {
        setPeopleData(jsonData)
      })
    })
  }
  useEffect(fetch_people, [])
  const defaultState = {
    firstName: "",
    lastName: ""
  }

  const [state, setState] = React.useState(defaultState)
  function handleChange(evt) {
    const value = evt.target.value;
    setState({
      ...state,
      [evt.target.name]: value
    });
  }

  const submitPerson = event => {
    event.preventDefault() // don't redirect the page
    fetch('/api/import_person', {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(state),
    }).then(() => {
      fetch_people()
      setState(defaultState)
    })
  }

  return (
    <div>
      <form onSubmit={submitPerson}>
        <label>
          First name: 
          <input
            type="text"
            name="firstName"
            value={state.firstName}
            onChange={handleChange}
          />
        </label>
        <label>
          Last name: 
          <input
            type="text"
            name="lastName"
            value={state.lastName}
            onChange={handleChange}
          />
        </label>
        <button type="submit">Submit Person</button>
      </form>
      <table>
        <tbody>
          <tr>
            <td>firstName</td>
            <td>lastName</td>
          </tr>
          {peopleData.map((person) => (
            <tr>
              <td>{person.firstName}</td>
              <td>{person.lastName}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}