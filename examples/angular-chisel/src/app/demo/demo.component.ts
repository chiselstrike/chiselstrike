import { Component, ElementRef, OnInit, ViewChild } from '@angular/core';

require('es6-promise').polyfill();
var originalFetch = require('isomorphic-fetch');
var fetch = require('fetch-retry')(originalFetch, {
    retries: 10,
    compress: true,
    retryDelay: function(attempt: any, error: any, response: any) {
      return Math.pow(2, attempt) * 1000;
    }
});

class Status {
  text: string = "";
  color: string = "black";
}

@Component({
  selector: 'app-demo',
  templateUrl: './demo.component.html',
  styleUrls: ['./demo.component.css']
})
export class DemoComponent implements OnInit {
  @ViewChild("fileInput")
  file_input: ElementRef<HTMLInputElement> = {} as ElementRef;
  files_to_upload: Array<File> = [];

  status: Status = new Status();
  upload_progress: number = 0;
  upload_button_disabled: boolean = false;
  
  img_data: Array<string> = [];

  constructor() { }

  async ngOnInit() {
    this.resetFileInputForm();
    this.setStatusOK("Ready");
    await this.loadRandomImages();
  }

  setStatusError(message: string) {
    this.status.text = message;
    this.status.color = "red";
  }

  setStatusOK(message: string) {
    this.status.text = message;
    this.status.color = "green";
  }

  resetFileInputForm() {
    if (this.file_input) {
      this.file_input = {} as ElementRef;
    }
  }

  imagesSelected(target: EventTarget | null) {
    if (target === null) {
      return;
    }
    const files = (target as HTMLInputElement).files
    if (!files) {
      return;
    }
    this.files_to_upload = new Array<File>();
    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      this.files_to_upload.push(file);
    }
  }

  async upload() {
    try {
      this.upload_button_disabled = true;
      this.setStatusOK("Uploading...");

      if (this.files_to_upload.length == 0) {
        this.setStatusError("Choose the files to upload!");
        return;
      }

      for (let file of this.files_to_upload) {
        let form_data = new FormData();
        form_data.append(file.name, file);
        const response = await fetch("/api/import_images", {
          method: "PUT",
          headers: {},
          body: form_data,
        });
        if (!response.ok) {
          throw `Failed to import image '${file.name}'`;
        }
      }
        
      this.setStatusOK(`Uploaded Successfully!`);
      await this.loadRandomImages();
    } catch (e) {
      this.setStatusError(e);
    } finally {
      this.upload_button_disabled = false;
      this.resetFileInputForm();
    }
  }

  async uploadToServer(url: string, form_data: FormData) {
    try {
      this.setStatusOK(`Uploading...`);
      const response = await fetch(url, {
        method: "PUT",
        headers: {},
        body: form_data,
      });
      if (response.status == 200) {
        return false;
      }
      this.setStatusError(`Error: Failed to upload the data`);
    } catch (err) {
      this.setStatusError(`Error, not saved! ${err.status_text}`);
    }
    return true;
  }

  async loadRandomImages() {
    const response = await fetch("/api/get_random_images");
    const json_images = await response.json();

    this.img_data = [];
    for (const json_img of json_images) {
      this.img_data.push(json_img.data)
    }
  }
}
